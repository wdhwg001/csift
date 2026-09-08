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
//!   region csift's own record set cannot resolve. Claude Code drops these; csift keeps
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
//! FAIL-OPEN, always - and the reason is csift's OWN forensic step, not a gap in the
//! record set. Claude Code's walk STOPS at a `compact_boundary` (its `parentUuid` is
//! null and the loader never reads `logicalParentUuid`), so it never asks whether that
//! logical parent still exists. csift crosses the cut through exactly that field to keep
//! reading what the model saw before it - and on a real corpus 52 of 245 boundary
//! records, spread over 14 of 75 transcripts, name a `logicalParentUuid` that matches no
//! `uuid` on ANY line of their own file, because the pre-compaction records were deleted
//! from disk. Thirteen of those files end csift's walk on such a boundary. Everything
//! physically above that point is a region csift cannot resolve, and it is not empty.
//!
//! So the walk's terminating record is a FLOOR: an off-chain record physically above it
//! is `PreCut`, never `Abandoned` - csift declines to call a record abandoned in a region
//! it could not resolve. In that blind region the measured same-parent opener rule (an
//! opener with a LATER opener sharing its `parentUuid` was recalled and re-sent) still
//! applies, so draft detection never depends on the chain reaching that far.

use super::*;
use std::collections::{HashMap, HashSet};

mod branches;
mod leaf;
mod nodes;
mod relink;
mod walk;

pub(crate) use nodes::Builder;

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
    /// The newest `compact_boundary` the walk stopped at or stepped over; the disclosure
    /// `boundary_cut_line` is derived from it.
    pub boundary_cut: Option<usize>,
    /// The walk's terminating index - the floor below which `Abandoned` is claimed.
    #[allow(dead_code)]
    pub floor: usize,
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
    #[allow(dead_code)]
    #[must_use]
    pub fn build(records: &[Record], leaf_hint: Option<&str>) -> Chain {
        Self::build_by(records, |r| r, leaf_hint)
    }

    /// [`Chain::build`] over any wrapper (`&Record`, the search `Kept`) via a projector -
    /// the same shape [`group_turn_indices_deduped`] takes.
    #[must_use]
    pub fn build_by<T>(
        records: &[T],
        rec: impl Fn(&T) -> &Record,
        leaf_hint: Option<&str>,
    ) -> Chain {
        let mut b = Builder::new(records, &rec);
        let anchor = relink::apply(&mut b);
        b.index_children();
        let (leaf, source) = leaf::pick(&b, leaf_hint, anchor);
        let out = walk::run(&mut b, leaf);
        branches::finish(b, leaf, source, out)
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
