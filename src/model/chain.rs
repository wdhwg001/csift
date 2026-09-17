//! The SURVIVAL AXIS: Claude Code's own conversation-chain rule.
//!
//! Claude Code does not reconstruct a session by reading its jsonl top to bottom. It
//! loads every `user`/`assistant`/`attachment`/`system` record into a uuid map (LAST
//! occurrence of a uuid wins), picks ONE leaf, and walks `parentUuid` from that leaf to
//! the head. Only the records that walk reaches are the conversation; everything else
//! on disk was written, then left behind - a prompt recalled and re-typed, a turn the
//! operator rewound past, the whole history above a compaction boundary. That single
//! rule replaces the three special cases csift used to carry (the same-parent draft
//! heuristic, the replay-copy collapse, and the boundary's implicit cut) and answers a
//! question none of them could: which of a branch point's children is the conversation.
//!
//! Three states per record ([`Survival`]):
//! - `Live` - on the chain (or rescued onto it by the two membership rules below).
//! - `PreCut` - csift's forensic reading ABOVE a cut the loader stops at, plus any
//!   region csift's own walk could not resolve. Claude Code drops these; csift keeps
//!   reading them, because "what the model saw at the time" is the question a transcript
//!   archive answers. They are flagged, never hidden: every selector reaches them.
//! - `Abandoned` - off the chain in a region the chain DID resolve. An abandoned turn
//!   OPENER is a [`Kind::Draft`] (nothing ever answered it) or a [`Kind::Rewound`] (it
//!   drew a reply, and the conversation was later rewound past it).
//!
//! Membership is not plain ancestry. Two loader passes widen it and both are mirrored:
//! same-`message.id` assistant siblings of a chain assistant record - and the
//! `tool_result` carriers parented to them - are Live, and every NON-conversation
//! descendant of the leaf is Live.
//!
//! FAIL-OPEN, always - and the reason is where the walk STOPS. Claude Code's walk ends at
//! a `compact_boundary`: its `parentUuid` is null (275 of 275 boundary records over the 23
//! compacted files among the 82 top-level transcripts of one projects root) and the
//! loader never reads `logicalParentUuid`, the field naming the record the compaction
//! re-linked from. csift follows that field under a CROSSING BUDGET ([`CrossingBudget`],
//! the global `--cross-compactions` flag), because the conversation genuinely continues
//! across a compaction - the records above a cut are the same conversation, and a turn the
//! operator rewound past up there cannot be named until the walk reaches it - and a reader
//! asking what a session did wants the turns above the cut resolved rather than declared
//! unreadable. What the walk reaches above a crossed boundary takes the ordinary rules:
//! Live where the chain reaches it, Abandoned where it does not.
//!
//! The walk still ends ON a boundary two ways, and both leave a FLOOR: the budget ran out,
//! or the `logicalParentUuid` reached no record the map still holds (claim MISC-043: 52 of
//! 255 boundary records, over 14 of the 21 compacted transcripts, name a
//! `logicalParentUuid` matching no `uuid` on ANY line of their own file; a relink cut
//! removes more). An off-chain record physically above the floor is `PreCut`, never
//! `Abandoned` - csift declines to call a record abandoned in a region it could not
//! resolve - and [`CrossingReport`] states which cause it was, how many boundaries remain
//! above and the flag value that would reach them. In that blind region the measured
//! same-parent opener rule (an opener with a LATER opener sharing its `parentUuid` was
//! recalled and re-sent) still applies, so draft detection never depends on the chain
//! reaching that far.

use super::*;
use std::collections::{HashMap, HashSet};

mod branches;
mod budget;
mod leaf;
mod nodes;
mod relink;
mod view;
mod walk;

pub use budget::{CrossingBudget, CrossingReport, FloorCause, CROSS_COMPACTIONS_DEFAULT};
pub(crate) use nodes::Builder;
pub(crate) use nodes::ChainNode;
use nodes::Slot;
pub use view::{ChainView, TurnStamp};

/// Where the chain's leaf came from - disclosed, because the leaf decides the whole
/// answer and Claude Code's own pick has four gates on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeafSource {
    /// The `leafUuid` of the newest `last-prompt` line, resolved in the map.
    LastPrompt,
    /// Same, on a line that also carried `explicit:true` (a writer pinned it).
    Explicit,
    /// The file-order-last non-sidechain record (no usable `last-prompt` leaf).
    #[default]
    Tail,
    /// A `last-prompt` leaf was recorded but names no record in this file (a fork copy,
    /// a truncated file): the tail is used and the miss is stated.
    TailLeafAbsent,
    /// An `explicit:true` line carried a NULL leaf, which empties the leaf set outright.
    /// The loader then has no recorded leaf at all and falls back to the newest record by
    /// TIMESTAMP - not to the file-order tail, which is a different record whenever a
    /// write landed out of time order.
    Cleared,
}

impl LeafSource {
    /// The stable wire/text spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LeafSource::LastPrompt => "last-prompt",
            LeafSource::Explicit => "explicit",
            LeafSource::Tail => "tail",
            LeafSource::TailLeafAbsent => "tail (last-prompt leaf absent)",
            LeafSource::Cleared => "newest (leaf set cleared)",
        }
    }
}

/// One record's place in the surviving conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Survival {
    /// On the chain the loader walks (or rescued onto it).
    #[default]
    Live,
    /// Above a cut the loader stops at, or in a region csift could not resolve. Kept and
    /// selectable exactly as before - only flagged.
    PreCut,
    /// Off the chain, in a region the chain resolved. `root` is the index of the
    /// abandoned branch's topmost record (the branch head a reader should look at).
    Abandoned { root: usize },
}

impl Survival {
    /// The stable wire spelling (JSON `survival`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Survival::Live => "live",
            Survival::PreCut => "pre-cut",
            Survival::Abandoned { .. } => "abandoned",
        }
    }

    /// Did the model receive this record? `Abandoned` is the only no - a `PreCut` record
    /// WAS the conversation when it was written, which is the reading csift keeps.
    #[must_use]
    pub fn selectable(self) -> bool {
        !matches!(self, Survival::Abandoned { .. })
    }
}

/// What an ABANDONED turn-opener was. The discriminator is whether anything under it
/// ever answered: a recalled prompt has no assistant descendant, a rewound turn does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A prompt that was recalled into the input box (or otherwise left behind) and never
    /// drew a reply. `superseded_by` is the surviving opener that replaced it, when the
    /// two share a `parentUuid`.
    Draft { superseded_by: Option<usize> },
    /// A prompt that WAS answered, and the conversation was later rewound past it.
    /// `resend` is the surviving opener the conversation continued from instead.
    Rewound { resend: Option<usize> },
}

impl Kind {
    /// The surviving opener this one was replaced by, whichever kind it is.
    #[must_use]
    pub fn survivor(self) -> Option<usize> {
        match self {
            Kind::Draft { superseded_by } => superseded_by,
            Kind::Rewound { resend } => resend,
        }
    }

    /// The leaf an abandoned opener carries: a draft stays `user.unsent`, a rewound turn
    /// takes the leaf minted for it.
    #[must_use]
    pub fn class(self) -> Class {
        match self {
            Kind::Draft { .. } => Class::UserUnsent,
            Kind::Rewound { .. } => Class::UserRewound,
        }
    }
}

/// The uuid map the chain build already made, handed back rather than dropped: every
/// chain-admitted uuid in one transcript, pointing at the record index that WINS it.
///
/// It answers the one cross-record question a family fact keeps asking ("this record
/// NAMES a uuid; which line is that?"), and it answers it the way the loader would: the
/// LAST line carrying a uuid is the survivor, so an id that a compaction re-anchor
/// appended twice resolves to the copy a reader should be sent to, not to the dead one.
/// Borrowed from the records, so no uuid is copied and the index costs one move.
#[derive(Debug)]
pub struct UuidIndex<'a> {
    map: HashMap<&'a str, Slot>,
}

impl<'a> UuidIndex<'a> {
    /// Take ownership of the build's own map. Crate-internal: nothing outside the chain
    /// has one to hand over.
    pub(crate) fn from_map(map: HashMap<&'a str, Slot>) -> UuidIndex<'a> {
        UuidIndex { map }
    }
}

impl UuidIndex<'_> {
    /// The index of the record this uuid names, or `None` when no record here carries
    /// it, which is the common answer for a uuid naming another session, a wire id, or a
    /// record a pre-boundary deletion removed from disk.
    #[must_use]
    pub fn index_of(&self, uuid: &str) -> Option<usize> {
        self.map.get(uuid).map(|slot| slot.last)
    }

    /// How many distinct uuids the map holds - the instrument that says this is the
    /// build's own map rather than a second one built beside it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// The survival answer for one transcript's records, in the caller's own index space.
#[derive(Debug, Default)]
pub struct Chain {
    survival: Vec<Survival>,
    kinds: HashMap<usize, Kind>,
    replay_of: HashMap<usize, usize>,
    rewound_branch: HashSet<usize>,
    /// The record the walk started from, after the walk-up to a conversation record.
    /// Read by the unit matrix (which pins the leaf gates one shape at a time); kept as a
    /// named chain fact rather than an anonymous local - same rationale as the targeted
    /// allow on `Record`'s tolerance fields.
    #[allow(dead_code)]
    pub leaf_index: Option<usize>,
    pub leaf_source: LeafSource,
    /// The `compact_boundary` the walk STOPPED at, which is the floor whenever the floor is
    /// a boundary, and `None` when the walk ended at a head, a dangling parent or a cycle.
    /// The disclosure `boundary_cut_line` is derived from it, and it keeps the meaning it
    /// had before the crossing budget existed: the record the surviving conversation is cut
    /// at. A boundary the walk stepped OVER is counted in `boundaries_crossed` and named
    /// nowhere - naming it here would print "the conversation is cut at L<n>" for a cut
    /// csift got past, two lines above the floor it actually stopped at.
    pub boundary_cut: Option<usize>,
    /// The walk's terminating index - the floor below which `Abandoned` is claimed.
    pub floor: usize,
    /// Compaction boundaries the walk stepped over.
    pub boundaries_crossed: usize,
    /// The budget it ran under.
    pub budget: CrossingBudget,
    /// The walk met a boundary with no budget left.
    pub budget_exhausted: bool,
    /// Boundaries the walk did NOT cross (the floor's own included).
    pub boundaries_above_floor: usize,
    /// Why the walk ended where it did.
    pub floor_cause: FloorCause,
    /// Under `FloorCause::BudgetExhausted`, which of the three facts shut the door of the
    /// boundary the walk stopped at - or None when the door was open, which is the one case
    /// where one more unit of budget moves this floor.
    pub floor_shut: Option<FloorCause>,
    /// Records classified `Abandoned` (the footer + summary count).
    pub abandoned_records: usize,
    /// Abandoned openers with an assistant descendant.
    pub rewound_turns: usize,
    /// Abandoned openers without one (the `user.unsent` population).
    pub drafts: usize,
    /// Lines carrying a uuid a LATER line also carries (a compaction re-anchor's copies).
    pub replay_copies: usize,
}

impl Chain {
    /// Build the chain over one transcript's records IN FILE ORDER.
    ///
    /// `leaf_hint` OVERRIDES the leaf discovery for a caller that already knows the
    /// `last-prompt` leaf; passing `None` (the production path) makes the chain find it
    /// among the records themselves - a `last-prompt` spine row - which is also the only
    /// way to honour the loader's rule that a `compact_boundary` seen afterwards WIPES it.
    ///
    /// `budget` is how many compaction boundaries the walk may step over
    /// ([`CrossingBudget`]); it is never defaulted inside the chain, so every caller states
    /// the reading it wants and a hard-coded one is visible at the call site.
    #[allow(dead_code)]
    #[must_use]
    pub fn build(records: &[Record], leaf_hint: Option<&str>, budget: CrossingBudget) -> Chain {
        Self::build_by(records, |r| ChainNode::Full(r), leaf_hint, budget)
    }

    /// [`Chain::build`] over any wrapper (a `&Record`, a search `Row`, a stats row) via a
    /// projector to the ONE shape the walk reads - the same shape
    /// [`group_turn_indices_deduped`] takes. The projector is what lets a surface keep its
    /// full records and its narrow spine rows in separate vectors and still hand the chain
    /// one file-order list.
    #[must_use]
    pub fn build_by<T>(
        records: &[T],
        node: impl Fn(&T) -> ChainNode<'_>,
        leaf_hint: Option<&str>,
        budget: CrossingBudget,
    ) -> Chain {
        Self::build_by_indexed(records, node, leaf_hint, budget).0
    }

    /// [`Chain::build_by`], handing back the uuid map the build already made
    /// ([`UuidIndex`]) instead of dropping it.
    ///
    /// The map is the loader's own: every chain-admitted record's uuid, pointing at the
    /// line that WINS it. It is built on every chain build - the walk cannot run without
    /// it - so a caller that needs to resolve a uuid ONE RECORD NAMES pays nothing here
    /// beyond not dropping it, which is why this exists instead of a second index. The
    /// borrow is the records', so the index is usable exactly as long as they are.
    #[must_use]
    pub fn build_by_indexed<'a, T>(
        records: &'a [T],
        node: impl Fn(&T) -> ChainNode<'_>,
        leaf_hint: Option<&str>,
        budget: CrossingBudget,
    ) -> (Chain, UuidIndex<'a>) {
        let mut b = Builder::new(records, &node);
        let anchor = relink::apply(&mut b);
        b.index_children();
        let (leaf, source) = leaf::pick(&b, leaf_hint, anchor);
        let out = walk::run(&mut b, leaf, budget);
        branches::finish(b, leaf, source, out, budget)
    }

    /// The crossing disclosure, with the FLOOR's address filled in by the caller: the chain
    /// answers in record indices and only the surface knows which jsonl line and instant
    /// those are.
    #[must_use]
    pub fn crossing(&self, floor_line: Option<usize>, floor_ts: Option<String>) -> CrossingReport {
        CrossingReport {
            budget: self.budget,
            boundaries_crossed: self.boundaries_crossed,
            budget_exhausted: self.budget_exhausted,
            boundaries_above_floor: self.boundaries_above_floor,
            cause: Some(self.floor_cause),
            floor_shut: self.floor_shut,
            floor_line,
            floor_ts,
        }
    }

    /// This record's survival. An index past the end reads `Live` (nothing is claimed
    /// about a record the chain never saw).
    #[must_use]
    pub fn survival(&self, i: usize) -> Survival {
        self.survival.get(i).copied().unwrap_or_default()
    }

    /// The opener kind of an ABANDONED turn-opener; `None` for every other record.
    #[must_use]
    pub fn kind(&self, i: usize) -> Option<Kind> {
        self.kinds.get(&i).copied()
    }

    /// The index of the LATER line carrying this line's uuid - a compaction re-anchor
    /// re-appended the record and the last copy is the survivor. `None` when this line is
    /// the survivor (or carries no uuid).
    #[must_use]
    pub fn replay_of(&self, i: usize) -> Option<usize> {
        self.replay_of.get(&i).copied()
    }

    /// True when this record is on an abandoned branch whose head was ANSWERED - the
    /// `[rewound]` marker, as opposed to the plain `[abandoned]` one.
    #[must_use]
    pub fn on_rewound_branch(&self, i: usize) -> bool {
        self.rewound_branch.contains(&i)
    }

    /// Does this record open a turn as far as the chain is concerned? A replay copy never
    /// does (the survivor already did) and neither does an abandoned record (it belongs to
    /// no numbered turn). The caller still applies [`Record::opens_turn`].
    #[must_use]
    pub fn opens(&self, i: usize) -> bool {
        self.survival(i).selectable() && !self.replay_of.contains_key(&i)
    }

    /// The single leaf an abandoned opener carries at the SCAN layer (`user.unsent` /
    /// `user.rewound`), replacing whatever `classify` would have said. `None` for every
    /// record that is not an abandoned opener.
    #[must_use]
    pub fn opener_class(&self, i: usize) -> Option<Class> {
        self.kinds.get(&i).map(|k| k.class())
    }

    /// The surviving opener that replaced an abandoned one - the address the C-27 diff
    /// line is computed against.
    #[must_use]
    pub fn superseding(&self, i: usize) -> Option<usize> {
        self.kinds.get(&i).and_then(|k| k.survivor())
    }

    /// The head of this record's abandoned branch; `None` unless it is abandoned.
    #[must_use]
    pub fn abandoned_root(&self, i: usize) -> Option<usize> {
        match self.survival(i) {
            Survival::Abandoned { root } => Some(root),
            _ => None,
        }
    }
}
