//! The SURVIVAL AXIS for a surface whose byte prefilter keeps only part of the DAG.
//!
//! `search` and `show` splice [`crate::parse::spine_record`] rows straight into their own
//! record list, so the chain walks the whole DAG for free. The file-oriented surfaces
//! (`verbatim`, `files`, `recover`, `image`) keep a DIFFERENT slice of the transcript and
//! index everything by their own record positions, so they cannot simply widen that list.
//! [`ChainView`] closes the gap: it merges the surface's records with the spine rows of the
//! lines it dropped, runs the ONE chain, and answers back IN THE SURFACE'S OWN INDEX SPACE.
//! Turn numbers therefore agree with what `search` prints for the same transcript, which is
//! the whole point - a `verbatim` turn 7 and a `search … --count-by turn` row 7 are the same
//! turn.
//!
//! Two answers per record, and they are different questions. [`ChainView::survival`] says
//! whether the surviving conversation still reaches the record. [`ChainView::stamp`] says
//! where it sits in LIVE turn numbering - `Live(n)`, or `Abandoned` naming the branch head's
//! physical line, because an abandoned record belongs to no numbered turn.
//!
//! The extraction GROUPS ([`ChainView::groups`]) are what a per-turn extractor iterates: the
//! live turns in order, then one group per abandoned branch. A disk-event surface must walk
//! the abandoned groups too - an Edit on a rewound branch DID hit the disk - and the group
//! shape keeps each branch's own `tool_use`/`tool_result` join scope intact, exactly as a
//! turn does.

use super::*;
use crate::parse::SpineRow;

/// Where a record sits in LIVE turn numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStamp {
    /// Inside the numbered turn `n`.
    Live(usize),
    /// Off the surviving conversation, so inside NO numbered turn. `root_line` is the
    /// physical jsonl line of the abandoned branch's head - the address a reader follows.
    Abandoned { root_line: usize },
}

impl TurnStamp {
    /// The numbered turn, or `None` for an abandoned record (JSON `turn_index` is null there).
    #[must_use]
    pub fn index(self) -> Option<usize> {
        match self {
            TurnStamp::Live(n) => Some(n),
            TurnStamp::Abandoned { .. } => None,
        }
    }

    #[must_use]
    pub fn is_abandoned(self) -> bool {
        matches!(self, TurnStamp::Abandoned { .. })
    }

    /// The physical line of the abandoned branch head; `None` for a live record.
    #[must_use]
    pub fn root_line(self) -> Option<usize> {
        match self {
            TurnStamp::Live(_) => None,
            TurnStamp::Abandoned { root_line } => Some(root_line),
        }
    }

    /// The ONE text spelling every surface prints in a turn slot: `7`, or
    /// `abandoned (root L42)`.
    #[must_use]
    pub fn text(self) -> String {
        match self {
            TurnStamp::Live(n) => n.to_string(),
            TurnStamp::Abandoned { root_line } => format!("abandoned (root L{root_line})"),
        }
    }
}

/// The chain answered in one surface's own record index space (see the module doc).
#[derive(Debug, Default)]
pub struct ChainView {
    survival: Vec<Survival>,
    stamp: Vec<TurnStamp>,
    /// The live turn a record physically FOLLOWS - the numbered turn for a live record,
    /// the nearest preceding one for an abandoned record. It is an ORDERING key only (the
    /// `--turn` window and `--at @turn:` need one for every event); it is never printed,
    /// because an abandoned record has no numbered turn.
    order_turn: Vec<usize>,
    turns: Vec<Vec<usize>>,
    /// One entry per abandoned branch: `(branch head's jsonl line, its members)`, the
    /// members in file order, the branches ascending by head line.
    branches: Vec<(usize, Vec<usize>)>,
    /// The physical lines of the abandoned turn OPENERS (a recalled draft or a rewound
    /// turn), ascending - what a footer counts and points at.
    abandoned_openers: Vec<usize>,
}

impl ChainView {
    /// Build the view over one transcript's `(line, record)` pairs plus the SPINE rows of
    /// the lines the surface's prefilter dropped. Both slices must be in file order (every
    /// producer in the tree is); they are merged by line number.
    #[must_use]
    pub fn build(records: &[(usize, Record)], spine: &[SpineRow]) -> ChainView {
        let n = records.len();
        let total = n + spine.len();
        let mut combined: Vec<ChainNode<'_>> = Vec::with_capacity(total);
        let mut combined_line: Vec<usize> = Vec::with_capacity(total);
        // `origin[c]` = the surface index of combined row `c`, or `None` for a spine row.
        let mut origin: Vec<Option<usize>> = Vec::with_capacity(total);
        let (mut a, mut b) = (0usize, 0usize);
        while a < n || b < spine.len() {
            let take_record = match (records.get(a), spine.get(b)) {
                (Some((la, _)), Some(sb)) => *la <= sb.line(),
                (Some(_), None) => true,
                _ => false,
            };
            if take_record {
                combined.push(ChainNode::Full(&records[a].1));
                combined_line.push(records[a].0);
                origin.push(Some(a));
                a += 1;
            } else {
                combined.push(ChainNode::Spine(&spine[b]));
                combined_line.push(spine[b].line());
                origin.push(None);
                b += 1;
            }
        }

        let chain = Chain::build_by(&combined, |r| *r, None);
        let index_turns = group_turn_indices_chained(&combined, |r| *r, &chain);
        // (the projector is the identity: `combined` already holds the ONE node shape)

        let mut survival = vec![Survival::Live; n];
        let mut stamp = vec![TurnStamp::Live(0); n];
        let mut order_turn = vec![0usize; n];
        let mut turns: Vec<Vec<usize>> = Vec::with_capacity(index_turns.len());
        // The turn a combined row belongs to, so the ordering key can be filled forward.
        let mut turn_of_combined: Vec<Option<usize>> = vec![None; combined.len()];
        for (ti, group) in index_turns.iter().enumerate() {
            let mut own: Vec<usize> = Vec::new();
            for &c in group {
                turn_of_combined[c] = Some(ti);
                if let Some(i) = origin[c] {
                    own.push(i);
                    stamp[i] = TurnStamp::Live(ti);
                }
            }
            turns.push(own);
        }

        let mut branch_by_root: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        let mut abandoned_openers: Vec<usize> = Vec::new();
        let mut running = 0usize;
        for c in 0..combined.len() {
            if let Some(t) = turn_of_combined[c] {
                running = t;
            }
            let sv = chain.survival(c);
            if chain.kind(c).is_some() {
                abandoned_openers.push(combined_line[c]);
            }
            let Some(i) = origin[c] else { continue };
            survival[i] = sv;
            order_turn[i] = running;
            if let Some(root) = chain.abandoned_root(c) {
                let root_line = combined_line.get(root).copied().unwrap_or(combined_line[c]);
                stamp[i] = TurnStamp::Abandoned { root_line };
                branch_by_root.entry(root_line).or_default().push(i);
            }
        }

        ChainView {
            survival,
            stamp,
            order_turn,
            turns,
            branches: branch_by_root.into_iter().collect(),
            abandoned_openers,
        }
    }

    /// This surface record's survival. An index past the end reads `Live` (nothing is
    /// claimed about a record the view never saw), matching [`Chain::survival`].
    #[must_use]
    pub fn survival(&self, i: usize) -> Survival {
        self.survival.get(i).copied().unwrap_or_default()
    }

    /// This surface record's place in live turn numbering.
    #[must_use]
    pub fn stamp(&self, i: usize) -> TurnStamp {
        self.stamp.get(i).copied().unwrap_or(TurnStamp::Live(0))
    }

    /// The live turn this record physically follows - an ORDERING key for windows and
    /// cutoffs, never a printed turn number (see the field doc).
    #[must_use]
    pub fn order_turn(&self, i: usize) -> usize {
        self.order_turn.get(i).copied().unwrap_or(0)
    }

    /// The live turns, outer index = the 0-based turn number, members = surface record
    /// indices in file order. A turn whose records were ALL dropped by the surface's
    /// prefilter is present and empty, so the numbering never shifts.
    #[must_use]
    pub fn turns(&self) -> &[Vec<usize>] {
        &self.turns
    }

    /// The live turn COUNT - the domain a `--turn` spec resolves its open/from-end forms
    /// against.
    #[must_use]
    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    /// Every extraction group in one walk: the live turns in turn order, then one group per
    /// abandoned branch (ascending by branch-head line). Each group carries the stamp its
    /// rows take, so a per-turn extractor keeps its own join scope and its rows stay
    /// attributable.
    #[must_use]
    pub fn groups(&self) -> Vec<(TurnStamp, &[usize])> {
        let mut out: Vec<(TurnStamp, &[usize])> = self
            .turns
            .iter()
            .enumerate()
            .map(|(ti, g)| (TurnStamp::Live(ti), g.as_slice()))
            .collect();
        out.extend(self.branches.iter().map(|(root_line, g)| {
            (
                TurnStamp::Abandoned {
                    root_line: *root_line,
                },
                g.as_slice(),
            )
        }));
        out
    }

    /// The physical lines of the abandoned turn OPENERS, ascending.
    #[must_use]
    pub fn abandoned_openers(&self) -> &[usize] {
        &self.abandoned_openers
    }
}
