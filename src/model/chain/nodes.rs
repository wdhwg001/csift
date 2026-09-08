//! Admission + the uuid map: which records Claude Code's loader reasons about at all,
//! and which physical line wins when a uuid appears twice.

use super::*;

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
    pub(crate) recs: Vec<&'a Record>,
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
pub(crate) fn chain_admits(r: &Record) -> bool {
    if r.is_elicitation_marker() {
        return false;
    }
    if r.uuid.as_deref().is_none_or(str::is_empty) {
        return false;
    }
    matches!(
        r.r#type.as_deref(),
        Some("user" | "assistant" | "attachment" | "system")
    )
}

impl<'a> Builder<'a> {
    pub(crate) fn new<T>(records: &'a [T], rec: &impl Fn(&T) -> &Record) -> Builder<'a> {
        let recs: Vec<&'a Record> = records.iter().map(rec).collect();
        let n = recs.len();
        let mut admit = Vec::with_capacity(n);
        let mut parent: Vec<Option<&'a str>> = Vec::with_capacity(n);
        let mut map: HashMap<&'a str, Slot> = HashMap::with_capacity(n);
        let mut replay_of: HashMap<usize, usize> = HashMap::new();
        for (i, r) in recs.iter().copied().enumerate() {
            let ok = chain_admits(r);
            admit.push(ok);
            parent.push(r.parent_uuid.as_deref().filter(|s| !s.is_empty()));
            if !ok {
                continue;
            }
            let Some(uuid) = r.uuid.as_deref() else {
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
            if let Some(last) = recs[*i].uuid.as_deref().and_then(|u| map.get(u)) {
                *target = last.last;
            }
        }
        // Only a `type:"user"` record can open a turn (all four cases are user records),
        // so the expensive predicate runs on those alone.
        let opens = recs
            .iter()
            .map(|r| r.r#type.as_deref() == Some("user") && r.opens_turn())
            .collect();
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
        self.recs[i].uuid.as_deref()
    }

    pub(crate) fn is_conv(&self, i: usize) -> bool {
        matches!(self.recs[i].r#type.as_deref(), Some("user" | "assistant"))
    }

    pub(crate) fn is_boundary(&self, i: usize) -> bool {
        self.recs[i].r#type.as_deref() == Some("system")
            && self.recs[i].subtype.as_deref() == Some("compact_boundary")
    }

    /// A main-thread record. A `isSidechain:true` record belongs to a separate lane the
    /// main chain never contains, so the axis does not apply to it (it is never called
    /// abandoned).
    pub(crate) fn is_sidechain(&self, i: usize) -> bool {
        self.recs[i].is_sidechain == Some(true)
    }
}
