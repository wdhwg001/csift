//! Mutation + boundary extraction (structured tools + bash heuristics).

use super::*;

/// Extract the bare file mutations carried by a record slice — the SAME structured +
/// carrier-join + Bash-heuristic logic [`extract_mutations`] uses, but WITHOUT turn
/// tagging (no session id, no turn index). Reused by the subagent topology to compute a
/// node's files-changed over its own transcript ([`crate::subagent::build_topology`]),
/// so the two surfaces never diverge on what counts as a mutation. Carriers are joined
/// over the whole slice (a subagent transcript is one logical scope).
#[must_use]
pub fn mutations_in_records(records: &[Record]) -> Vec<FileMutation> {
    // Build the carrier join map once over the whole slice: tool_use_id → (filePath,
    // is_create). A subagent transcript is a single scope, so a global join is correct.
    let mut carriers: BTreeMap<String, (String, bool)> = BTreeMap::new();
    // tool_use_ids whose RESULT errored / was cancelled (`is_error:true`) — those ops never
    // landed, so they are not real mutations (mirrors `extract_mutations` + `recover::extract`).
    let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rec in records {
        for (id, file_path, is_create) in rec.carrier_create_paths() {
            carriers.insert(id, (file_path, is_create));
        }
        if let Some(blocks) = rec.blocks() {
            for b in blocks {
                if let crate::model::Block::ToolResult {
                    tool_use_id: Some(id),
                    is_error: Some(true),
                    ..
                } = b
                {
                    failed_ids.insert(id.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    for rec in records {
        // A record whose tool op errored / was cancelled mutated nothing — skip it.
        if tool_use_id_for(rec).is_some_and(|id| failed_ids.contains(&id)) {
            continue;
        }
        for mut m in rec.structured_tool_mutations() {
            if let Some(id) = tool_use_id_for(rec) {
                if let Some((carrier_path, is_create)) = carriers.get(&id) {
                    m.is_create = *is_create;
                    if m.path.is_empty() {
                        m.path = carrier_path.clone();
                    }
                }
            }
            if m.path.is_empty() {
                continue;
            }
            out.push(m);
        }
        if let Some(cmd) = rec.bash_command() {
            for bm in parse_bash_mutations(cmd) {
                out.push(FileMutation {
                    path: bm.path,
                    op: FileOp::BashMutation,
                    timestamp_utc: rec.timestamp.clone(),
                    is_create: bash_verb_is_create(bm.verb),
                });
            }
        }
    }
    out
}

/// Heuristic create-vs-touch guess for a Bash mutation verb. A verb that names a fresh
/// output target (`>` truncate, `mkdir`/`touch`/`tee`/`cp`/`mv`/`install`/`ln`/`rsync`
/// dest, a download to a path, a `dd`/`zip`/`tar`-create/flag-specified output) is treated
/// as a create; an append (`>>`, `tee-a`), `rm`, `sed -i`, `mv-from`, and `git` are NOT.
/// (`emit_tar` only emits on a `-c`/`--create` flag, so the `tar` verb is unconditionally a
/// create; `tee-a` is `tee --append`, the non-truncating sibling of `tee`, mirroring `>>`
/// vs `>`.) Lexical-only, so it is just a heuristic (its `FileOp::BashMutation`
/// is_heuristic() gates the label everywhere).
pub(crate) fn bash_verb_is_create(verb: &str) -> bool {
    matches!(
        verb,
        "mkdir"
            | "touch"
            | "tee"
            | ">"
            | "cp"
            | "mv"
            | "install"
            | "ln"
            | "rsync"
            | "curl"
            | "wget"
            | "dd"
            | "zip"
            | "tar"
            | "flag-output"
    )
}

/// Delimit turns over the parsed records, then for each turn extract structured + Bash
/// mutations and JOIN the structured ones to their carriers for accurate `is_create`.
pub(crate) fn extract_mutations(
    session_id: &str,
    records: &[Record],
    line_nos: &[usize],
) -> Vec<TaggedMutation> {
    let index_turns = group_turn_indices_deduped(records, |r| r);
    let mut out = Vec::new();

    for (turn_index, idxs) in index_turns.iter().enumerate() {
        // Build the carrier join map for this turn: tool_use_id → (filePath, is_create).
        let mut carriers: BTreeMap<String, (String, bool)> = BTreeMap::new();
        // tool_use_ids whose RESULT was an error (`is_error:true`) — a failed Edit/Write, or a
        // Write `Cancelled: parallel tool call … errored` when a sibling op in the same batch
        // failed. The op NEVER landed, so it must NOT be counted as a real mutation: a `files`
        // `write:1` on a cancelled Write contradicts `recover` (which correctly finds no
        // history) and is a forensic FALSE POSITIVE ("did this session write X?"). Same
        // `failed_ids` gate `recover::extract` already applies; computed per turn (the result
        // block sits in the same turn as its call).
        let mut failed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for &i in idxs {
            for (id, file_path, is_create) in records[i].carrier_create_paths() {
                carriers.insert(id, (file_path, is_create));
            }
            if let Some(blocks) = records[i].blocks() {
                for b in blocks {
                    if let crate::model::Block::ToolResult {
                        tool_use_id: Some(id),
                        is_error: Some(true),
                        ..
                    } = b
                    {
                        failed_ids.insert(id.clone());
                    }
                }
            }
        }

        for &i in idxs {
            let rec = &records[i];

            // A record whose tool op errored / was cancelled never mutated anything — skip its
            // structured AND heuristic-Bash mutations (the op's INPUT is a phantom, not a write).
            if tool_use_id_for(rec).is_some_and(|id| failed_ids.contains(&id)) {
                continue;
            }

            // Structured (authoritative) mutations, enriched from the carrier join.
            for mut m in rec.structured_tool_mutations() {
                // The carrier join keys on the tool_use's own id; find it on this record.
                if let Some(id) = tool_use_id_for(rec) {
                    if let Some((carrier_path, is_create)) = carriers.get(&id) {
                        m.is_create = *is_create;
                        if m.path.is_empty() {
                            m.path = carrier_path.clone();
                        }
                    }
                }
                if m.path.is_empty() {
                    continue;
                }
                out.push(TaggedMutation {
                    session_id: session_id.to_string(),
                    // is_subagent / parent default here; `scan_one_file` stamps the real
                    // per-file values once (the path-derived discriminator lives there).
                    is_subagent: false,
                    parent_session_id: session_id.to_string(),
                    turn_index,
                    line_no: line_nos.get(i).copied().unwrap_or(0),
                    mutation: m,
                });
            }

            // Bash (heuristic) mutations.
            if let Some(cmd) = rec.bash_command() {
                for bm in parse_bash_mutations(cmd) {
                    out.push(TaggedMutation {
                        session_id: session_id.to_string(),
                        is_subagent: false,
                        parent_session_id: session_id.to_string(),
                        turn_index,
                        line_no: line_nos.get(i).copied().unwrap_or(0),
                        mutation: FileMutation {
                            path: bm.path,
                            op: FileOp::BashMutation,
                            timestamp_utc: rec.timestamp.clone(),
                            // Bash create-vs-overwrite is NOT knowable lexically — this is
                            // a heuristic flag; the op's is_heuristic() gates the label.
                            is_create: bash_verb_is_create(bm.verb),
                        },
                    });
                }
            }
        }
    }
    out
}

/// True when a `tool_result` body is the `File has been modified since read` harness error
/// (the file changed OUTSIDE the tool stream — prettier/linter/git/etc. — and a fresh Read is
/// demanded). Mirrors `recover::classify_integrity_error`'s `ModifiedSinceRead` arm; kept local
/// so `files` doesn't depend on `recover`'s internals.
pub(crate) fn is_modified_since_read(content: &serde_json::Value) -> bool {
    let text = crate::model::tool_result_content_text(content);
    text.contains("has been modified since read") || text.contains("File has been modified")
}

/// Extract the Edit-before-Read boundaries a session hit on each file: an Edit/Write rejected
/// with `File has been modified since read` (the file changed outside the Read/Write/Edit
/// stream). Attribution: the error `tool_result`'s `tool_use_id` matches the rejected op, whose
/// `file_path` lives on its tool_use record (even though the op never landed) — so a per-turn
/// `id → path` map (built from EVERY edit/write tool_use, failed or not) names the file. The
/// jsonl line is taken from `line_nos` (aligned with `records` by index).
pub(crate) fn extract_boundaries(
    session_id: &str,
    records: &[Record],
    line_nos: &[usize],
) -> Vec<TaggedBoundary> {
    let index_turns = group_turn_indices_deduped(records, |r| r);
    let mut out = Vec::new();
    for (turn_index, idxs) in index_turns.iter().enumerate() {
        // id → file_path for every Edit/Write tool_use in this turn (incl. failed ones — the
        // rejected edit's INPUT still carries its file_path).
        let mut tool_use_path: BTreeMap<String, String> = BTreeMap::new();
        for &i in idxs {
            if let Some(id) = tool_use_id_for(&records[i]) {
                if let Some(m) = records[i]
                    .structured_tool_mutations()
                    .into_iter()
                    .find(|m| !m.path.is_empty())
                {
                    tool_use_path.entry(id).or_insert(m.path);
                }
            }
        }
        for &i in idxs {
            let Some(blocks) = records[i].blocks() else {
                continue;
            };
            for b in blocks {
                if let crate::model::Block::ToolResult {
                    tool_use_id: Some(id),
                    is_error: Some(true),
                    content: Some(content),
                } = b
                {
                    if is_modified_since_read(content) {
                        if let Some(path) = tool_use_path.get(id) {
                            out.push(TaggedBoundary {
                                session_id: session_id.to_string(),
                                is_subagent: false,
                                parent_session_id: session_id.to_string(),
                                path: path.clone(),
                                line_no: line_nos.get(i).copied().unwrap_or(0),
                                turn_index,
                                kind: "modified_since_read",
                                timestamp_utc: records[i].timestamp.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// The first tool_use block's `id` on this record (the structured-mutation join key).
/// A record's structured mutations all come from its tool_use blocks; in real data a
/// single assistant record carries one file-mutating tool_use, so the first id is the
/// join key. Returns `None` when there is no tool_use id.
pub(crate) fn tool_use_id_for(rec: &Record) -> Option<String> {
    let blocks = rec.blocks()?;
    for block in blocks {
        if let crate::model::Block::ToolUse { id: Some(id), .. } = block {
            return Some(id.clone());
        }
    }
    None
}
