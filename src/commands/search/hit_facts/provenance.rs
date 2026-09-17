//! The PROVENANCE half of the stamping pass: which message a unit corresponds to.
//!
//! Split from `hit_facts.rs` at the 600-line structure gate, along the seam the file
//! already had. What stays there is stamped from the RECORD alone - the unmodeled
//! shape, the queue pair, the address, the survival verdict. What is here needs the
//! record's PLACE: the turn it sits in and the neighbour it names, which is the one
//! question this family answers and the reason the table and the edges are one file.

use super::*;

/// One turn's ROOT table: which record opened it, and how far up the parent chain each
/// record of the turn sits from that opener.
///
/// Built ONCE per turn and only for a turn that produced a hit, because most turns of a
/// scanned file produce none and a table built for every turn would be a per-record cost on
/// the whole file to answer a question about the records that matched.
///
/// A TABLE and not a per-hit walk, which is the difference between linear and quadratic.
/// `parentUuid` is stamped from the running tail of the transcript (claim CLS-049), so a
/// turn is a CHAIN: the record at depth d is d hops from the opener, and walking per hit
/// costs the sum of the depths - N-squared over a turn whose records all match. Filling the
/// table forward instead costs one hop per record, because a parent precedes its child in
/// file order and its own answer is therefore already in the table.
pub(crate) struct TurnRoots {
    /// The position of the record that OPENS this turn, or `None` when the group has none.
    ///
    /// It is NOT always position 0: the shared grouper folds a synthetic pre-user lead into
    /// the first real turn (`model::grouping`, the `synthetic_lead` fold), and a real
    /// transcript almost always opens with one - a SessionStart hook attachment, a
    /// `<local-command-caveat>` on a `/clear`-minted file, a compaction boundary. Reading
    /// position 0 as the opener named that lead as the message every unit of turn 0
    /// corresponds to, and left the human message itself reading three hops below it.
    opener: Option<usize>,
    /// Hops from each position UP to the opener; `None` where the parent chain does not
    /// reach it.
    hops: Vec<Option<u32>>,
}

impl TurnRoots {
    /// Fill the table in ONE forward pass over the turn's rows.
    pub(crate) fn build(turn: &Turn<'_>) -> TurnRoots {
        // Spine rows are in the position map too: an attachment chain runs through lines the
        // prefilter dropped, and a table that could not see them would stop at the first one.
        // A uuid a turn carries TWICE (a compaction re-anchor's replay copy) maps to the
        // LATER position, which is the loader's own rule; a child pointing at it then fails
        // the `p < i` test below and reads as unreached rather than as a guessed distance.
        let positions: HashMap<&str, usize> = turn
            .records
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.uuid().map(|u| (u, i)))
            .collect();
        let opener = turn.records.iter().position(|r| r.node().opens_turn());
        let mut hops: Vec<Option<u32>> = vec![None; turn.records.len()];
        if let Some(o) = opener {
            hops[o] = Some(0);
            for i in o + 1..turn.records.len() {
                let Some(parent) = turn.records[i].node().parent_uuid() else {
                    continue;
                };
                // A parent precedes its child in file order, so its answer is already in the
                // table. A link that does not move UP is a cycle or the inversion claim
                // REC-075 measured (four corpus joins where a tool result's line sits 1-2
                // lines above its call); either way the honest answer is no distance.
                let Some(&p) = positions.get(parent) else {
                    continue;
                };
                if p < i {
                    hops[i] = hops[p].map(|h| h + 1);
                }
            }
        }
        TurnRoots { opener, hops }
    }

    /// WHICH MESSAGE the record at `pos` corresponds to.
    ///
    /// FOUR answers and each is a fact. The unit IS the opener, so nothing above it in this
    /// turn. The chain reaches the opener, so the address and the distance. The grouping
    /// places the unit under the opener but the links do not carry it there, so the address
    /// and no distance - csift knows WHICH turn without being able to say how it got there.
    /// Or there is no opener above the unit at all: it is part of the synthetic lead folded
    /// into turn 0, or of an abandoned branch whose head opens no turn, and naming the
    /// opener BELOW it would point a reader the wrong way down the file.
    pub(crate) fn link(&self, turn: &Turn<'_>, pos: usize) -> crate::model::RootLink {
        let Some(o) = self.opener.filter(|&o| o < pos) else {
            return if self.opener == Some(pos) {
                crate::model::RootLink::opener()
            } else {
                crate::model::RootLink::no_opener()
            };
        };
        let opener = &turn.records[o];
        crate::model::RootLink::under(
            opener.line_no(),
            opener.uuid().map(str::to_string),
            self.hops.get(pos).copied().flatten(),
        )
    }
}

/// The ENGINE's own `for` edges for one hit: what this unit corresponds to, one hop up.
///
/// TWO relations, and a unit can carry BOTH because they are different facts. A unit that
/// ANSWERS a call names it with the API's own `tool_use_id`, which is the only relation in
/// the vocabulary that is not a uuid relation - hence the [`crate::model::ProvIndex`] beside
/// the chain's uuid map. Every record also hangs off `parentUuid`, which is where the record
/// SITS rather than a target its producer chose, so that edge is [`Basis::Position`]. On the
/// ordinary turn the two name the SAME record and only the specific word renders; on a LATE
/// result - one that lands in a later turn, parented there - they name different records and
/// dropping either would lose a relation the record states.
///
/// FOUR leaves answer a call, not one. Two are the plain result views (`agent.tool.result`
/// and the `agent.communication.inbox` that supersedes it on a subagent return), and two are
/// the USER-FACING duals: an answered AskUserQuestion and a refused tool call are the
/// person's own message and a tool result at once, so they render under `user.answer` /
/// `user.rejection` while still riding the carrier that names the call. The dual's hit
/// carries that id ([`crate::model::Record::answered_call_id`]) for exactly this edge -
/// `set_pairing` keys on the class and leaves both outside its domain, so the id moves
/// provenance and nothing else.
///
/// ONE RELATION, ONE EDGE decides which is dropped when they coincide: `parent` stands down
/// when a family already named that uuid with a more specific word (`blocked`, `redoes`,
/// `fires`, `attached_to`, `wraps`) or when it resolves to the very LINE `result_of` already
/// named. Two words for one relation is a vocabulary a reader has to learn twice, which is
/// the failure the shared builder exists to prevent, from the other side.
pub(crate) fn engine_edges(
    h: &Hit,
    parent_uuid: Option<&str>,
    tools: &ToolIndex,
    lines: &dyn crate::model::LineOfUuid,
) -> Vec<HitFact> {
    let mut out: Vec<HitFact> = Vec::new();
    let mut named_line: Option<usize> = None;
    if matches!(
        h.class,
        Some(Class::AgentToolResult | Class::CommInbox | Class::UserAnswer | Class::UserRejection)
    ) {
        if let Some(id) = h.tool_use_id.as_deref() {
            named_line = tools.prov.tool_use_at(id);
            out.push(record_text::for_edge_at(
                crate::model::EdgeKind::ResultOf,
                Basis::Key,
                Some(id),
                None,
                named_line,
            ));
        }
    }
    let Some(parent) = parent_uuid else {
        return out;
    };
    let line = lines.line_of_uuid(parent);
    if line.is_some() && line == named_line {
        return out;
    }
    if h.facts
        .iter()
        .any(|f| matches!(f, HitFact::For { target, .. } if target.uuid.as_deref() == Some(parent)))
    {
        return out;
    }
    out.push(record_text::for_edge_at(
        crate::model::EdgeKind::Parent,
        Basis::Position,
        Some(parent),
        None,
        line,
    ));
    out
}

/// Stamp the provenance answer onto every hit a record just produced: the turn root, and
/// the engine's own one-hop edge.
pub(crate) fn backfill_provenance(
    hits: &mut [Hit],
    turn: &Turn<'_>,
    pos: usize,
    roots: &TurnRoots,
    tools: &ToolIndex,
    lines: &dyn crate::model::LineOfUuid,
) {
    let root = roots.link(turn, pos);
    let parent_uuid = turn.records[pos].node().parent_uuid();
    for h in hits.iter_mut() {
        h.for_root = root.clone();
        h.facts.extend(engine_edges(h, parent_uuid, tools, lines));
    }
}
