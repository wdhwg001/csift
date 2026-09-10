//! Admission + the uuid map: which records Claude Code's loader reasons about at all,
//! and which physical line wins when a uuid appears twice - plus [`ChainNode`], the ONE
//! shape the walk reads a line through.
//!
//! Two very different rows reach the chain. A full [`Record`] is what the surface's own
//! prefilter admitted; a [`SpineRow`] is the chain-structural lift of a line it dropped,
//! a fraction of the width and carrying no payload at all. The walk needs the same dozen
//! fields from either, so it reads both through one two-arm enum: the surfaces keep their
//! two vectors apart (the whole point of the narrow row) and the chain never has to know
//! which vector a row came from.

use super::*;
use crate::parse::SpineRow;

/// One line as the chain reads it: a full record, or the narrow structural row of a line
/// the surface's prefilter dropped.
///
/// Every accessor answers what a [`Record`] would have answered. A spine row carries no
/// `message`, so the three message-derived answers ([`ChainNode::opens_turn`],
/// [`ChainNode::message_id`], [`ChainNode::has_tool_result`]) are the same `false`/`None`
/// a message-less record gave - which is what the pre-0.12.1 shape produced, since a spine
/// row WAS a message-less record.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ChainNode<'a> {
    Full(&'a Record),
    Spine(&'a SpineRow),
}

impl<'a> ChainNode<'a> {
    /// The full record, or `None` for a chain-only spine row.
    pub(crate) fn full(self) -> Option<&'a Record> {
        match self {
            ChainNode::Full(r) => Some(r),
            ChainNode::Spine(_) => None,
        }
    }

    pub(crate) fn kind(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.r#type.as_deref(),
            ChainNode::Spine(s) => Some(s.kind_str()),
        }
    }

    pub(crate) fn subtype(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.subtype.as_deref(),
            ChainNode::Spine(s) => s.subtype(),
        }
    }

    pub(crate) fn uuid(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.uuid.as_deref(),
            ChainNode::Spine(s) => s.uuid.as_deref(),
        }
    }

    pub(crate) fn parent_uuid(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.parent_uuid.as_deref(),
            ChainNode::Spine(s) => s.parent_uuid.as_deref(),
        }
    }

    pub(crate) fn logical_parent_uuid(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.logical_parent_uuid.as_deref(),
            ChainNode::Spine(s) => s.logical_parent_uuid(),
        }
    }

    pub(crate) fn leaf_uuid(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.leaf_uuid.as_deref(),
            ChainNode::Spine(s) => s.leaf_uuid(),
        }
    }

    pub(crate) fn explicit(self) -> Option<bool> {
        match self {
            ChainNode::Full(r) => r.explicit,
            ChainNode::Spine(s) => s.explicit(),
        }
    }

    pub(crate) fn is_sidechain(self) -> Option<bool> {
        match self {
            ChainNode::Full(r) => r.is_sidechain,
            ChainNode::Spine(s) => s.is_sidechain,
        }
    }

    /// The RAW timestamp string. Parsed by the caller with the same
    /// [`crate::timez::epoch_ms`] either kind would have gone through.
    pub(crate) fn timestamp(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.timestamp.as_deref(),
            ChainNode::Spine(s) => s.timestamp.as_deref(),
        }
    }

    pub(crate) fn compact_metadata(self) -> Option<&'a serde_json::Value> {
        match self {
            ChainNode::Full(r) => r.compact_metadata.as_ref(),
            ChainNode::Spine(s) => s.compact_metadata(),
        }
    }

    pub(crate) fn message_id(self) -> Option<&'a str> {
        match self {
            ChainNode::Full(r) => r.message.as_ref().and_then(|m| m.id.as_deref()),
            ChainNode::Spine(_) => None,
        }
    }

    pub(crate) fn has_tool_result(self) -> bool {
        match self {
            ChainNode::Full(r) => r
                .blocks()
                .is_some_and(|bs| bs.iter().any(|x| matches!(x, Block::ToolResult { .. }))),
            ChainNode::Spine(_) => false,
        }
    }

    pub(crate) fn is_elicitation_marker(self) -> bool {
        match self {
            ChainNode::Full(r) => r.is_elicitation_marker(),
            ChainNode::Spine(_) => false,
        }
    }

    /// Only a `type:"user"` record can open a turn (all four cases are user records), so
    /// the expensive predicate runs on those alone - and never on a spine row, which has
    /// no `message` to open one with.
    pub(crate) fn opens_turn(self) -> bool {
        match self {
            ChainNode::Full(r) => r.r#type.as_deref() == Some("user") && r.opens_turn(),
            ChainNode::Spine(_) => false,
        }
    }

    /// The compaction-MODE pair (C-33), which `search` builds over the same merged row
    /// list: a boundary names itself here, a summary yields its mode. A spine row is
    /// never a summary (a summary is a `role:user` record, so every prefilter admits it
    /// whole), so the second answer is exactly the `false` a message-less record gave.
    pub(crate) fn is_compact_boundary(self) -> bool {
        self.kind() == Some("system") && self.subtype() == Some("compact_boundary")
    }

    pub(crate) fn summary_compaction_mode(self) -> Option<SummarizeMode> {
        match self {
            ChainNode::Full(r) => r.summary_compaction_mode(),
            ChainNode::Spine(_) => None,
        }
    }
}

/// The two physical lines a uuid can occupy: the FIRST (Claude Code's Map keeps the
/// insertion position there, which is what its pre-boundary cut compares) and the LAST
/// (whose record object wins, because `map.set` overwrites the value).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) first: usize,
    pub(crate) last: usize,
}

/// The mutable state every build stage shares. Borrows the caller's records; the string
/// slices in `parent` point either into a record or at another record's uuid (the
/// preserved-messages relink rewrites them).
pub(crate) struct Builder<'a> {
    pub(crate) recs: Vec<ChainNode<'a>>,
    pub(crate) admit: Vec<bool>,
    pub(crate) parent: Vec<Option<&'a str>>,
    pub(crate) map: HashMap<&'a str, Slot>,
    /// Removed from the map by the preserved-messages cut - unreachable, and PreCut.
    pub(crate) removed: Vec<bool>,
    pub(crate) on_chain: Vec<bool>,
    /// On the chain but ABOVE a boundary the walk stepped over.
    pub(crate) precut: Vec<bool>,
    pub(crate) visited: Vec<bool>,
    /// Earlier line -> the later line carrying the same uuid.
    pub(crate) replay_of: HashMap<usize, usize>,
    /// Parent uuid -> the chain's own child, recorded during the walk. The surviving
    /// sibling an abandoned opener was replaced by.
    pub(crate) chain_child: HashMap<&'a str, usize>,
    /// Lazily built ascending (epoch-ms, index) table for the 5 s parent repair.
    pub(crate) ts_sorted: Option<Vec<(i64, usize)>>,
    /// [`Record::opens_turn`] per record, computed ONCE. Four stages ask it, and on a
    /// transcript with tens of thousands of records re-deriving it (it re-reads content,
    /// peer sections and answer markers) costs more than the whole rest of the build.
    pub(crate) opens: Vec<bool>,
    /// Parent index -> children, in CSR form: `child_at[child_off[p]..child_off[p+1]]`.
    /// A `HashMap<usize, Vec<usize>>` here meant one heap allocation per parent.
    pub(crate) child_off: Vec<usize>,
    pub(crate) child_at: Vec<usize>,
}

/// Is this a record Claude Code's loader puts in its uuid map? Four types with a
/// non-empty string uuid. A csift elicitation-sidecar record is EXCLUDED: it is a
/// pending elicitation csift merged in, has no place in any on-disk DAG, and marking it
/// off-chain would call a live question abandoned.
pub(crate) fn chain_admits(r: ChainNode<'_>) -> bool {
    if r.is_elicitation_marker() {
        return false;
    }
    if r.uuid().is_none_or(str::is_empty) {
        return false;
    }
    matches!(
        r.kind(),
        Some("user" | "assistant" | "attachment" | "system")
    )
}

impl<'a> Builder<'a> {
    pub(crate) fn new<T>(records: &'a [T], node: &impl Fn(&T) -> ChainNode<'_>) -> Builder<'a> {
        let recs: Vec<ChainNode<'a>> = records.iter().map(node).collect();
        let n = recs.len();
        let mut admit = Vec::with_capacity(n);
        let mut parent: Vec<Option<&'a str>> = Vec::with_capacity(n);
        let mut map: HashMap<&'a str, Slot> = HashMap::with_capacity(n);
        let mut replay_of: HashMap<usize, usize> = HashMap::new();
        for (i, r) in recs.iter().copied().enumerate() {
            let ok = chain_admits(r);
            admit.push(ok);
            parent.push(r.parent_uuid().filter(|s| !s.is_empty()));
            if !ok {
                continue;
            }
            let Some(uuid) = r.uuid() else {
                continue;
            };
            match map.get_mut(uuid) {
                Some(slot) => {
                    replay_of.insert(slot.last, i);
                    slot.last = i;
                }
                None => {
                    map.insert(uuid, Slot { first: i, last: i });
                }
            }
        }
        // An n-way replay (three copies of one block) points EVERY earlier line at the
        // final survivor, not at its immediate successor - the survivor is what the
        // loader's map holds and what an address should reach.
        for (i, target) in &mut replay_of {
            if let Some(last) = recs[*i].uuid().and_then(|u| map.get(u)) {
                *target = last.last;
            }
        }
        let opens = recs.iter().map(|r| r.opens_turn()).collect();
        Builder {
            recs,
            admit,
            parent,
            map,
            removed: vec![false; n],
            on_chain: vec![false; n],
            precut: vec![false; n],
            visited: vec![false; n],
            replay_of,
            chain_child: HashMap::new(),
            ts_sorted: None,
            opens,
            child_off: Vec::new(),
            child_at: Vec::new(),
        }
    }

    /// Build the CSR child index over every admitted survivor line, once the relink has
    /// settled every `parentUuid`.
    pub(crate) fn index_children(&mut self) {
        let n = self.len();
        let mut parent_of: Vec<Option<usize>> = vec![None; n];
        let mut counts = vec![0usize; n + 1];
        for (i, slot) in parent_of.iter_mut().enumerate() {
            if !self.admit[i] || !self.is_survivor(i) {
                continue;
            }
            if let Some(p) = self.parent[i].and_then(|p| self.resolve(p)) {
                *slot = Some(p);
                counts[p] += 1;
            }
        }
        let mut off = vec![0usize; n + 1];
        let mut acc = 0usize;
        for i in 0..n {
            off[i] = acc;
            acc += counts[i];
        }
        off[n] = acc;
        let mut cursor = off.clone();
        let mut at = vec![0usize; acc];
        for (i, p) in parent_of.iter().enumerate() {
            if let Some(p) = *p {
                at[cursor[p]] = i;
                cursor[p] += 1;
            }
        }
        self.child_off = off;
        self.child_at = at;
    }

    /// The children of record `i` (empty before [`Builder::index_children`] runs).
    pub(crate) fn children_of(&self, i: usize) -> &[usize] {
        match (self.child_off.get(i), self.child_off.get(i + 1)) {
            (Some(&a), Some(&b)) => &self.child_at[a..b],
            _ => &[],
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.recs.len()
    }

    /// The record a uuid resolves to: the LAST line carrying it, unless the cut removed
    /// it from the map.
    pub(crate) fn resolve(&self, uuid: &str) -> Option<usize> {
        let slot = self.map.get(uuid)?;
        (!self.removed[slot.last]).then_some(slot.last)
    }

    /// Is this line the survivor for its uuid (not an earlier replay copy)?
    pub(crate) fn is_survivor(&self, i: usize) -> bool {
        self.admit[i] && !self.replay_of.contains_key(&i)
    }

    pub(crate) fn uuid(&self, i: usize) -> Option<&'a str> {
        self.recs[i].uuid()
    }

    pub(crate) fn is_conv(&self, i: usize) -> bool {
        matches!(self.recs[i].kind(), Some("user" | "assistant"))
    }

    pub(crate) fn is_boundary(&self, i: usize) -> bool {
        self.recs[i].is_compact_boundary()
    }

    /// A main-thread record. A `isSidechain:true` record belongs to a separate lane the
    /// main chain never contains, so the axis does not apply to it (it is never called
    /// abandoned).
    pub(crate) fn is_sidechain(&self, i: usize) -> bool {
        self.recs[i].is_sidechain() == Some(true)
    }
}
