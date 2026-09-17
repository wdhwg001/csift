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
#[derive(Debug)]
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

#[cfg(test)]
thread_local! {
    /// How many root TABLES this thread has built - the instrument behind the "once per
    /// TURN, not once per pass" pin.
    ///
    /// Counted inside [`TurnRoots::build`] and nowhere else. A counter that observed the
    /// cell's own `is_none()` would be a SECOND implementation of the property it checks,
    /// and measurably a useless one: replacing the cell's body with an unconditional
    /// rebuild left such a counter reading 1, because the slot was still filled once.
    ///
    /// THREAD-LOCAL rather than global: the unit tests run in parallel in one process and
    /// several of them build tables, so a shared counter would read another test's work.
    /// The turn loop that fills it runs on the calling thread whenever the per-file
    /// fan-out is off, which is how the test drives it.
    pub(crate) static TURN_ROOT_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl TurnRoots {
    /// Fill the table in ONE forward pass over the turn's rows.
    pub(crate) fn build(turn: &Turn<'_>) -> TurnRoots {
        #[cfg(test)]
        TURN_ROOT_BUILDS.with(|n| n.set(n.get() + 1));
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

/// The ONE [`TurnRoots`] a turn's passes share, built on the turn's FIRST hit and not
/// before.
///
/// TWO passes can need the table - the matched hits, and the siblings under `--siblings` -
/// and each used to hold an `Option<TurnRoots>` of its own, so a turn with both built the
/// same table twice. "Built on a turn's first hit" is the cost this axis is stated at, and
/// the second build buys nothing: the table is a function of the turn, which neither pass
/// changes. The cell is owned by the caller that owns the turn, so the sentence is true of
/// the TURN rather than of each pass.
#[derive(Debug, Default)]
pub(crate) struct TurnRootsCell {
    table: Option<TurnRoots>,
}

impl TurnRootsCell {
    /// The table, building it on first use. What the cell is FOR is counted at the build
    /// ([`TURN_ROOT_BUILDS`]), not here: a counter on this side would agree with itself.
    pub(crate) fn get(&mut self, turn: &Turn<'_>) -> &TurnRoots {
        self.table.get_or_insert_with(|| TurnRoots::build(turn))
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
/// ONE RELATION, ONE EDGE decides which is dropped when they coincide, and the test is on
/// the TARGET rather than on who named it: `parent` stands down when ANY OTHER edge already
/// on this unit names that same uuid, or when it resolves to the very LINE `result_of` just
/// named. A FAMILY's word is the common case (`blocked`, `redoes`, `fires`, `attached_to`,
/// `wraps`), and TWO of the engine's own five rendering relations do it too - `pairs` on a
/// compaction summary and `repairs` on a paired resume placeholder both name the record
/// above, which is exactly what `parentUuid` names there. Measured on the landmark
/// transcript, that is 83 records: `parent` 169,180 -> 169,097, against 80 summaries and 3
/// paired placeholders. Two words for one relation is a vocabulary a reader has to learn
/// twice, which is the failure the shared builder exists to prevent, from the other side.
pub(crate) fn engine_edges(
    h: &Hit,
    parent_uuid: Option<&str>,
    tool_lines: &crate::model::ProvIndex,
    lines: &dyn crate::model::LineOfUuid,
) -> Vec<HitFact> {
    let mut out: Vec<HitFact> = Vec::new();
    let mut named_line: Option<usize> = None;
    if matches!(
        h.class,
        Some(Class::AgentToolResult | Class::CommInbox | Class::UserAnswer | Class::UserRejection)
    ) {
        if let Some(id) = h.tool_use_id.as_deref() {
            named_line = tool_lines.tool_use_at(id);
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

/// The FOUR relations the SURVIVAL AXIS already answers, as `for` edges.
///
/// Each one is a value [`crate::model::Chain`] computed for the record anyway and a field the
/// hit already carries: `replay_copy_of`, the `[abandoned]` / `[rewound]` marker, the
/// superseding line the draft-distance line names. The field stays exactly what it was - this
/// ADDS the machine-followable form beside it, so a consumer walking `for[]` reaches the same
/// four neighbours a reader reaches by eye, under words the vocabulary declares.
///
/// The BASIS splits, and the split is the honest part. `replays` is [`Basis::Key`]: the two
/// lines carry the SAME uuid, which is the key the loader itself joins on. The other three are
/// [`Basis::Position`]: a draft is recognised by a later opener sharing its `parentUuid`, a
/// rewound turn by what hangs below it, a branch head by where the chain stopped reaching -
/// readings of the file's shape, not targets a producer named.
///
/// They render NOTHING ([`EdgeKind::rendered_in_label`]), so this is called only where a
/// reader projects or censuses them, and a text scan that does neither renders every byte
/// without paying for any of it.
pub(crate) fn survival_edges(
    chain: &crate::model::Chain,
    idx: usize,
    rows: &[Row<'_>],
) -> Vec<HitFact> {
    // Every target below is a row the CHAIN located, and the chain locates a record by its
    // uuid - so the uuid is present by construction and the line is the row's own. A line of
    // 0 is the merged elicitation-sidecar record, which has no physical line at all: the edge
    // then renders its uuid rather than pointing a reader at `L0`.
    let edge = |kind, basis, i: usize| -> Option<HitFact> {
        let row = rows.get(i)?;
        let line = (row.line_no() > 0).then(|| row.line_no());
        Some(record_text::for_edge_at(
            kind,
            basis,
            row.uuid(),
            None,
            line,
        ))
    };
    let mut out: Vec<HitFact> = Vec::new();
    out.extend(
        chain
            .replay_of(idx)
            .and_then(|s| edge(crate::model::EdgeKind::Replays, Basis::Key, s)),
    );
    out.extend(match chain.kind(idx) {
        Some(crate::model::Kind::Draft { superseded_by }) => {
            superseded_by.and_then(|s| edge(crate::model::EdgeKind::Supersedes, Basis::Position, s))
        }
        Some(crate::model::Kind::Rewound { resend }) => {
            resend.and_then(|s| edge(crate::model::EdgeKind::Continues, Basis::Position, s))
        }
        None => None,
    });
    // The branch HEAD, and never on the head itself: an edge from a record to itself names
    // no neighbour, and the head already says what it is through its own `user.unsent` /
    // `user.rewound` leaf.
    out.extend(
        chain
            .abandoned_root(idx)
            .filter(|&root| root != idx)
            .and_then(|root| edge(crate::model::EdgeKind::Branches, Basis::Position, root)),
    );
    out
}

/// The THREE relations a COMPACTION boundary and its summary state about each other and
/// about the region the event touched.
///
/// They sit with the engine rather than in a family because no family owns
/// `harness.compaction`: the engine already reads the boundary's `compactMetadata` for the
/// hit's own `compact_metadata` and `mode` fields, and a second reader of the same object
/// would be a second answer to the same question.
///
/// `pairs` is the event's own two halves, and it is the one relation here whose BASIS
/// depends on the record. A boundary carrying `preservedSegment` names the summary outright
/// in `anchorUuid`, which is a key its producer wrote (measured: on one projects root, 275
/// of 275 boundaries carry `preservedSegment`, and on all 275 the `anchorUuid` resolves to
/// the `isCompactSummary` record). An older boundary that carries only the four original
/// scalars names nothing, and there the pairing is POSITION - the first summary after the
/// boundary and before the next one. A SUMMARY names no boundary on any key at all, so its
/// half of the edge is always position, and its honest empty says the TRUE reason: not that
/// the record lacks a key (no summary has one) but that no boundary precedes it in this
/// file - a clone head, whose boundary lives in the session it was copied from.
///
/// `precedes` is the `logicalParentUuid` the boundary carries in place of the null
/// `parentUuid` every boundary has - the true predecessor the compaction re-linked to. It
/// resolves to nothing on the shape claim MISC-043 measured, where the predecessor was
/// deleted from disk with the region it belonged to, and the builder's `not in this file`
/// is the honest answer there rather than a dropped edge.
///
/// `preserves` names the head and the TAIL of the segment the compaction KEPT, as two
/// edges. Two rather than one naming a range, because an edge names ONE neighbour and both
/// ends are addresses a reader fetches; a range would need a fifth target shape for a
/// relation that has exactly two useful points. What the summary REPLACED is the complement
/// of that segment and no key on either record names it, so no edge claims it - the
/// inventory records that absence rather than inventing a target for it.
fn compaction_edges(class: Class, rec: &Record, env: &ClassifyEnv<'_>) -> Vec<HitFact> {
    let edge = |kind, basis, uuid: Option<&str>| {
        let line = uuid.and_then(|u| env.lines.line_of_uuid(u));
        record_text::for_edge_at(kind, basis, uuid, None, line)
    };
    if class == Class::CompactionSummary {
        let Some(boundary) = env.summarize.paired_with(rec.uuid.as_deref()) else {
            return vec![HitFact::ForUnknown {
                kind: crate::model::EdgeKind::Pairs,
                reason: "no compaction boundary precedes this summary in this file".to_string(),
            }];
        };
        return vec![edge(
            crate::model::EdgeKind::Pairs,
            Basis::Position,
            Some(boundary),
        )];
    }
    if class != Class::CompactionBoundary {
        return Vec::new();
    }
    let segment = |member: &str| -> Option<&str> {
        rec.compact_metadata
            .as_ref()?
            .get("preservedSegment")?
            .get(member)?
            .as_str()
    };
    // The anchor is only the summary on the path that HAS one to point at. Claude Code's
    // builder takes the anchor as an argument, and its two callers pass different things:
    // the compaction path passes the last summary message's uuid, while the `/rewind`
    // summarize path passes the BOUNDARY's own uuid whenever the direction is not `up_to`.
    // An anchor equal to this record is a self-edge, which names no neighbour - so it falls
    // back to the position pairing exactly as an older boundary with no segment at all does.
    let (anchor, basis) = match segment("anchorUuid").filter(|a| Some(*a) != rec.uuid.as_deref()) {
        Some(a) => (Some(a), Basis::Key),
        None => (
            env.summarize.paired_with(rec.uuid.as_deref()),
            Basis::Position,
        ),
    };
    let mut out = vec![
        edge(crate::model::EdgeKind::Pairs, basis, anchor),
        edge(
            crate::model::EdgeKind::Precedes,
            Basis::Key,
            rec.logical_parent_uuid.as_deref(),
        ),
    ];
    out.extend(
        ["headUuid", "tailUuid"]
            .into_iter()
            .filter_map(segment)
            .map(|u| edge(crate::model::EdgeKind::Preserves, Basis::Key, Some(u))),
    );
    out
}

/// The dangling prompt a RESUME REPAIR placeholder was spliced in to answer.
///
/// When Claude Code resumes a transcript whose last loaded record is a `type:"user"` one it
/// appends a repair PROMPT and splices a stand-in assistant record under it. The two gates
/// differ, so the pair is not always a pair: a placeholder whose parent is an interrupt
/// marker or a slash-command wrapper is the same loader text with no prompt in front of it.
/// `Record::resume_paired` is the ONE writer of that verdict and the hit already carries it,
/// so the edge asks IT rather than re-testing the parent - the field and the relation cannot
/// then disagree about what a placeholder closes.
///
/// [`Basis::Position`], because the link is the `parentUuid` the append path stamps from the
/// running tail rather than a target the loader chose to name. An UNPAIRED placeholder gets
/// no edge at all and keeps the engine's `parent`: its parent is a real record and a real
/// relation, it is simply not a repair.
fn resume_edge(rec: &Record, kept: &Kept, env: &ClassifyEnv<'_>) -> Option<HitFact> {
    if rec.resume_paired(&env.ctx_for(kept)) != Some(true) {
        return None;
    }
    let uuid = rec.parent_uuid.as_deref();
    Some(record_text::for_edge_at(
        crate::model::EdgeKind::Repairs,
        Basis::Position,
        uuid,
        None,
        uuid.and_then(|u| env.lines.line_of_uuid(u)),
    ))
}

/// The CALL a `<task-notification>` pulse reports on, for EITHER carrier.
///
/// The pulse reaches a transcript two ways - as an ordinary `type:"user"` record and, far
/// less often, wrapped in an isMeta notification FRAME - and only the second is an isMeta
/// injection. Reading the relation in the family that owns the frame therefore answered for
/// 93 of the 3,997 pulse-carrying records on one projects root, and said nothing about the
/// other 3,904. It is read here instead, off the record's own text through the one reader
/// ([`Record::pulse_tool_use_id`]), so both carriers get the same answer.
///
/// [`Basis::Key`]: the producer wrote the tag to say which call this pulse is about. The
/// value is a `tool_use` id rather than a record uuid, so it resolves through the tool index
/// (2,449 of the 2,457 tagged pulses name a call of the same file).
///
/// A pulse with NO tag - about a third of them, a monitor cadence tick or a session start's
/// orphan reconciliation - answers the honest empty with that as the reason, never a silent
/// absence. Joining THOSE through their `<task-id>` means reading `toolUseResult` for every
/// tool result in the file, which section 7 keeps off a scan; it is a stated follow-on with
/// its share measured rather than a gap.
fn reports_edge(rec: &Record, env: &ClassifyEnv<'_>) -> HitFact {
    let Some(id) = rec.pulse_tool_use_id() else {
        return HitFact::ForUnknown {
            kind: crate::model::EdgeKind::Reports,
            reason: "the pulse names no tool-use-id".to_string(),
        };
    };
    let line = env.tool_lines.tool_use_at(&id);
    record_text::for_edge_at(
        crate::model::EdgeKind::Reports,
        Basis::Key,
        Some(&id),
        None,
        line,
    )
}

/// The relations the ENGINE states about a record because no family owns its leaf.
///
/// They RENDER, unlike the engine's one-hop pair and the survival four: each names a target
/// nothing else on the unit already gives as an address. So they are built whatever the
/// reader asked for, and each is gated on the unit's own leaf - a hit of any other leaf pays
/// one comparison and leaves with an empty vector, which allocates nothing.
fn record_edges(class: Class, kept: &Kept, env: &ClassifyEnv<'_>) -> Vec<HitFact> {
    let rec = &kept.rec;
    match class {
        Class::CompactionBoundary | Class::CompactionSummary => compaction_edges(class, rec, env),
        Class::ResumePlaceholder => resume_edge(rec, kept, env).into_iter().collect(),
        Class::NotificationWorkflow
        | Class::NotificationMonitor
        | Class::NotificationSubagent
        | Class::NotificationBackgroundCommand
        | Class::NotificationTask => vec![reports_edge(rec, env)],
        _ => Vec::new(),
    }
}

/// Stamp the provenance answer onto every hit a record just produced: the turn root, the
/// relations the engine reads off the record itself, and the engine's own one-hop edge.
pub(crate) fn backfill_provenance(
    hits: &mut [Hit],
    turn: &Turn<'_>,
    pos: usize,
    roots: &TurnRoots,
    env: &ClassifyEnv<'_>,
) {
    let root = roots.link(turn, pos);
    let parent_uuid = turn.records[pos].node().parent_uuid();
    let kept = turn.records[pos].kept();
    for h in hits.iter_mut() {
        // The ROOT is on every unit under every format: the label zone prints it.
        h.for_root = root.clone();
        if let (Some(class), Some(kept)) = (h.class, kept) {
            h.facts.extend(record_edges(class, kept, env));
        }
        // The two ONE-HOP edges ride JSON alone, so they are built only where a reader
        // projects or censuses them - the same gate the four survival relations take,
        // stamped beside the survival FIELDS where those belong. Nothing about the output
        // moves either way: a text scan renders no tag for `parent` or `result_of` and
        // never could (`EdgeKind::rendered_in_label` is false for exactly the six), and
        // what is saved is two string allocations and two map lookups per emitted hit.
        if env.wants_engine_edges {
            h.facts
                .extend(engine_edges(h, parent_uuid, env.tool_lines, env.lines));
        }
    }
}
