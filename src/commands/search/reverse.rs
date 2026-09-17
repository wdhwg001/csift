//! The REVERSE provenance query: everything that is FOR one record, transitively.
//!
//! Every other provenance surface answers forward - this unit points THERE. `--for` runs
//! the arrows the other way: name a record and get back the reply it drew, the results of
//! the calls it made, the notification a background task it launched sent two turns later,
//! and whatever hangs off any of those in turn.
//!
//! It is a CLOSURE and not a filter, which is what makes it worth a pass of its own: a
//! record is in the answer when it is for the address OR for something already in the
//! answer, so the set grows until it stops.
//!
//! ONE FORWARD PASS computes it, and that is a bound with a known cost. A parent is stamped
//! from the running tail of the transcript (claim CLS-049), so every ancestor of a row has
//! been read by the time the row is, and the parent arm is exact. The CALL arm is not: claim
//! REC-075 measured four corpus joins where a tool result's line sits 1-2 lines ABOVE its
//! call, and such a result is read before the call that would have admitted it and is
//! therefore MISSED - four joins out of 243,386 resolved within one file. A fixpoint would
//! close it and would cost a second pass over every row of every file in scope for a shape
//! that occurs at that rate, so the pass stays single and the limit is written down here.
//!
//! The cost is paid ONLY under the flag: nothing here runs on an ordinary scan.

use super::*;

/// What a `--for` address names.
///
/// TWO forms, and the difference is the ID LAW csift teaches everywhere else: a LINE is
/// per-FILE, so `L42` means line 42 of each transcript the scope holds and is meant to be
/// used with a target that names one; a UUID is global, so it finds its record in whichever
/// scanned transcript carries it.
///
/// The CLOSURE is per FILE under either form: [`for_closure`] runs once per transcript over
/// that transcript's rows, and both frontiers it grows - the uuids and the call ids - hold
/// only what that file states. So a uuid address finds its record wherever it sits and the
/// walk then stays inside that one file. Joining a closure ACROSS transcripts is a declared
/// follow-on, not something this pass half-does.
#[derive(Debug, Clone)]
pub(crate) enum ForAddr {
    /// The record at this 1-based jsonl line, in each scanned transcript.
    Line(usize),
    /// The record carrying this uuid, wherever it is.
    Uuid(String),
}

impl ForAddr {
    /// Parse the `--for <ADDR>` value: `L<n>` (or a bare number) for a line, anything else
    /// as a uuid.
    ///
    /// The bare number is accepted because `--for 42` is what a reader types after reading
    /// `L42` off a hit line, and refusing it would be a grammar lesson rather than an
    /// answer. A uuid is taken as written: csift does not check its shape here, because an
    /// address that resolves to nothing already reports itself as nothing found.
    pub(crate) fn parse(s: &str) -> Result<ForAddr> {
        // TRIMMED first: `--for ' 42 '` is a line, not a uuid that happens to have spaces
        // in it. No id csift resolves carries one, so the trim can lose nothing.
        let s = s.trim();
        let digits = s
            .strip_prefix('L')
            .or_else(|| s.strip_prefix('l'))
            .unwrap_or(s);
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            let n: usize = digits
                .parse()
                .map_err(|_| anyhow::anyhow!("--for: line number out of range: {s}"))?;
            if n == 0 {
                anyhow::bail!("--for: line numbers are 1-based, got {s}");
            }
            return Ok(ForAddr::Line(n));
        }
        if s.is_empty() {
            anyhow::bail!("--for: an address is required - `L<line>` or a record uuid");
        }
        Ok(ForAddr::Uuid(s.to_string()))
    }

    /// The address as the summary reports it back.
    pub(crate) fn as_str(&self) -> String {
        match self {
            ForAddr::Line(n) => format!("L{n}"),
            ForAddr::Uuid(u) => u.clone(),
        }
    }
}

/// The transitive closure of "is for the addressed record", as the set of LINES to emit.
///
/// THREE ways a row joins, and each is one of the provenance relations read structurally
/// rather than through a built edge (the edges are built per HIT, and the closure has to
/// run before any hit exists):
/// - its `parentUuid` names a member, which carries `parent` and every family relation
///   built on the parent link (`attached_to`, `blocked`, `redoes`, `fires`, `wraps`);
/// - it carries a `tool_result` answering a member's call, which is `result_of`;
/// - it names a member's call by id in its own text or payload, which is how a
///   notification pulse (`reports`) and an attachment (`for_tool_use`) point at one.
///
/// A row that joins contributes its OWN uuid and its own `tool_use` ids, which is what
/// makes the walk transitive rather than one hop.
///
/// The ADDRESSED record is in the set. A reader asking what a message drew wants to see
/// the message at the head of the answer, and leaving it out would make the first hit look
/// like the seed when it is already a consequence.
pub(crate) fn for_closure(rows: &[Row<'_>], addr: &ForAddr) -> BTreeSet<usize> {
    let mut lines: BTreeSet<usize> = BTreeSet::new();
    let mut uuids: HashSet<&str> = HashSet::new();
    let mut calls: HashSet<&str> = HashSet::new();
    for row in rows {
        let Some(kept) = row.kept() else {
            // A SPINE row carries no message, so it can never be emitted - but it is a
            // chain node, and a run of attachments hangs off one. Carrying its uuid into
            // the frontier is what lets the walk cross it instead of stopping there.
            if is_member_spine(*row, &uuids) {
                if let Some(u) = row.uuid() {
                    uuids.insert(u);
                }
            }
            continue;
        };
        let seed = match addr {
            ForAddr::Line(n) => kept.line_no == *n,
            ForAddr::Uuid(u) => kept.rec.uuid.as_deref() == Some(u.as_str()),
        };
        if !seed && !joins(kept, &uuids, &calls) {
            continue;
        }
        if kept.line_no > 0 {
            lines.insert(kept.line_no);
        }
        if let Some(u) = kept.rec.uuid.as_deref() {
            uuids.insert(u);
        }
        for id in tool_use_ids(&kept.rec) {
            calls.insert(id);
        }
    }
    lines
}

/// Does this SPINE row hang off a member? A spine row has a parent link and nothing else,
/// which is exactly the one question the closure asks of it.
fn is_member_spine(row: Row<'_>, uuids: &HashSet<&str>) -> bool {
    row.node().parent_uuid().is_some_and(|p| uuids.contains(p))
}

/// Does this record join the closure through any of the three structural relations?
///
/// THE PARENT ARM STOPS AT A TURN BOUNDARY, and without that the closure is not a closure
/// at all. Every record carries a parent link, including the one that OPENS the next turn,
/// so a walk that followed it unconditionally would swallow the whole rest of the file from
/// any address: the person's next message, its reply, and so on to the end. A new message
/// is not FOR the record above it - that is what a turn boundary means - so an opener joins
/// only through a relation it actually states, which is how a `<task-notification>`
/// reporting on a call inside the set still comes back while the person's next question
/// does not.
fn joins(kept: &Kept, uuids: &HashSet<&str>, calls: &HashSet<&str>) -> bool {
    if !kept.rec.opens_turn()
        && kept
            .rec
            .parent_uuid
            .as_deref()
            .is_some_and(|p| uuids.contains(p))
    {
        return true;
    }
    if let Some(blocks) = kept.rec.blocks() {
        for b in blocks {
            if let Block::ToolResult {
                tool_use_id: Some(id),
                ..
            } = b
            {
                if calls.contains(id.as_str()) {
                    return true;
                }
            }
        }
    }
    names_a_call(kept, calls)
}

/// Does this record NAME a member's call by id - the pulse that reports on it, or the
/// attachment written for it?
///
/// Both carry the id as text rather than as a block: a `<task-notification>` puts it in a
/// `<tool-use-id>` tag inside its own message, and an attachment puts it under the payload
/// key `toolUseID`. Neither is a record uuid, which is why the forward edges for them are
/// `reports` and `for_tool_use` rather than `parent`.
fn names_a_call(kept: &Kept, calls: &HashSet<&str>) -> bool {
    if calls.is_empty() {
        return false;
    }
    if let Some(id) = kept
        .rec
        .attachment_value()
        .and_then(|v| v.get("toolUseID"))
        .and_then(serde_json::Value::as_str)
    {
        if calls.contains(id) {
            return true;
        }
    }
    kept.rec
        .raw_message_text()
        .and_then(|s| crate::model::extract_xml_tag(&s, "tool-use-id"))
        .is_some_and(|id| calls.contains(id.as_str()))
}

/// Every `tool_use` block id this record carries - the calls it MADE, which its results
/// and its notifications will name.
fn tool_use_ids(rec: &Record) -> Vec<&str> {
    let Some(blocks) = rec.blocks() else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::ToolUse { id: Some(id), .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// How many distinct RECORDS the reverse query returned - the `for_of.records` number.
///
/// Records and not hits: one record emits a hit per matching section, and a closure that
/// reported sections would count a text-plus-tool_use assistant turn twice. It is the same
/// grouping every census counts by, for the same reason.
pub(crate) fn reverse_record_count(exchanges: &[Exchange]) -> usize {
    exchanges
        .iter()
        .map(|ex| record_groups(&ex.hits).len())
        .sum()
}
