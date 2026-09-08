//! Turning the walk into the three survival states, then reading the abandoned forest:
//! its branch heads, which of them were ANSWERED, and which surviving opener replaced
//! each one.
//!
//! Two guards keep the answer honest where csift's own forensic step could not land. A
//! record physically ABOVE the walk's terminating record sits in a region the chain did
//! not resolve - most often because the boundary csift tried to step over names a
//! `logicalParentUuid` that is no longer on disk (claim MISC-043) - so it is `PreCut`,
//! never `Abandoned`. And inside that blind region the measured same-parent opener rule
//! still applies: an opener with a LATER opener sharing its `parentUuid` was recalled and
//! re-sent, which is a fact about the two records alone and needs no chain.

use super::*;

pub(crate) fn finish(
    b: Builder<'_>,
    leaf: Option<usize>,
    source: LeafSource,
    walked: walk::Walked,
) -> Chain {
    let n = b.len();
    let survival = (0..n).map(|i| base_state(&b, i, walked.floor)).collect();
    let mut out = Chain {
        survival,
        kinds: HashMap::new(),
        replay_of: b.replay_of.clone(),
        rewound_branch: HashSet::new(),
        leaf_index: leaf,
        leaf_source: source,
        boundary_cut: walked.boundary,
        floor: walked.floor,
        abandoned_records: 0,
        rewound_turns: 0,
        drafts: 0,
        replay_copies: b.replay_of.len(),
    };
    let has_reply = assistant_below(&b);
    blind_region_openers(&b, &mut out, walked.floor);
    assign_roots(&b, &mut out);
    assign_kinds(&b, &mut out, &has_reply);
    // A replay copy is the SAME logical record as the line that carries its uuid later,
    // so it inherits that line's answer rather than reading as an abandoned branch.
    let inherits: Vec<(usize, Survival)> = out
        .replay_of
        .iter()
        .map(|(&i, &s)| (i, out.survival(s)))
        .collect();
    for (i, s) in inherits {
        out.survival[i] = s;
    }
    out.abandoned_records = out
        .survival
        .iter()
        .filter(|s| matches!(s, Survival::Abandoned { .. }))
        .count();
    out.drafts = out
        .kinds
        .values()
        .filter(|k| matches!(k, Kind::Draft { .. }))
        .count();
    out.rewound_turns = out.kinds.len() - out.drafts;
    out
}

/// The state before the abandoned forest is read: everything the chain reached is Live
/// (PreCut above a boundary it stepped over), everything above the floor is PreCut, and
/// the rest is a candidate for abandonment.
fn base_state(b: &Builder<'_>, i: usize, floor: usize) -> Survival {
    if !b.admit[i] || b.is_sidechain(i) {
        // Outside the loader's model (a promoted non-record line, a sidechain lane, a
        // merged elicitation marker): the axis says nothing about it.
        return Survival::Live;
    }
    if b.on_chain[i] {
        return if b.precut[i] {
            Survival::PreCut
        } else {
            Survival::Live
        };
    }
    if b.removed[i] || i < floor {
        return Survival::PreCut;
    }
    Survival::Abandoned { root: i }
}

/// The blind-region rescue of the draft population: inside `PreCut`, an opener with a
/// LATER opener sharing its `parentUuid` is still the recalled version.
fn blind_region_openers(b: &Builder<'_>, out: &mut Chain, floor: usize) {
    // The survivor is the LAST opener sharing a `parentUuid`, wherever it sits - a draft
    // is usually recalled just below the floor and re-sent just above it, so restricting
    // the comparison to the blind region would see the draft and never its resend.
    let mut last_by_parent: HashMap<&str, usize> = HashMap::new();
    for i in 0..b.len() {
        if !b.opens[i] || out.replay_of.contains_key(&i) || !out.survival[i].selectable() {
            continue;
        }
        if let Some(p) = b.parent[i] {
            last_by_parent.insert(p, i);
        }
    }
    let superseded: Vec<usize> = (0..b.len())
        .filter(|&i| {
            i < floor
                && matches!(out.survival[i], Survival::PreCut)
                && b.opens[i]
                && !out.replay_of.contains_key(&i)
                && b.parent[i].is_some_and(|p| last_by_parent.get(p).is_some_and(|&s| s > i))
        })
        .collect();
    for i in superseded {
        out.survival[i] = Survival::Abandoned { root: i };
    }
}

/// Give every abandoned record its branch head: the topmost ancestor that is itself
/// abandoned. Memoized upward walk, cycle-guarded.
fn assign_roots(b: &Builder<'_>, out: &mut Chain) {
    let n = b.len();
    let mut root: Vec<Option<usize>> = vec![None; n];
    let mut path: Vec<usize> = Vec::new();
    for i in 0..n {
        if !matches!(out.survival[i], Survival::Abandoned { .. }) {
            continue;
        }
        path.clear();
        let mut cur = i;
        let head = loop {
            if let Some(r) = root[cur] {
                break r;
            }
            path.push(cur);
            match b.parent[cur].and_then(|p| b.resolve(p)) {
                Some(p)
                    if matches!(out.survival[p], Survival::Abandoned { .. })
                        && !path.contains(&p) =>
                {
                    cur = p;
                }
                _ => break cur,
            }
        };
        for &k in &path {
            root[k] = Some(head);
            out.survival[k] = Survival::Abandoned { root: head };
        }
    }
}

/// Decide each abandoned OPENER's kind and name the surviving opener that replaced it,
/// then stamp the `[rewound]` branch membership.
fn assign_kinds(b: &Builder<'_>, out: &mut Chain, has_reply: &[bool]) {
    let openers: Vec<usize> = (0..b.len())
        .filter(|&i| {
            matches!(out.survival(i), Survival::Abandoned { .. })
                && b.opens[i]
                && !out.replay_of.contains_key(&i)
        })
        .collect();
    // The surviving opener per parent uuid, in ONE pass: a per-opener rescan would be
    // O(records x abandoned openers), which on a session with hundreds of recalled drafts
    // is the whole scan's cost over again.
    let mut last_opener: HashMap<&str, usize> = HashMap::new();
    for k in 0..b.len() {
        if out.survival(k).selectable() && !out.replay_of.contains_key(&k) && b.opens[k] {
            if let Some(p) = b.parent[k] {
                last_opener.insert(p, k);
            }
        }
    }
    let mut rewound_heads: Vec<usize> = Vec::new();
    for i in openers {
        let survivor = surviving_sibling(b, out, &last_opener, i);
        let kind = if has_reply[i] {
            rewound_heads.push(i);
            Kind::Rewound { resend: survivor }
        } else {
            Kind::Draft {
                superseded_by: survivor,
            }
        };
        out.kinds.insert(i, kind);
    }
    if rewound_heads.is_empty() {
        return;
    }
    let heads: HashSet<usize> = rewound_heads.into_iter().collect();
    for i in 0..b.len() {
        if let Survival::Abandoned { root } = out.survival(i) {
            if heads.contains(&root) || heads.contains(&i) {
                out.rewound_branch.insert(i);
            }
        }
    }
}

/// Does an assistant record hang BELOW each record? The one fact that separates a recalled
/// prompt from a rewound turn, and a pure DAG fact that needs no chain.
///
/// Computed for every record in ONE descending pass rather than per opener: a transcript
/// records a parent before its children, so walking indices downwards visits a record only
/// after all of its children, and each answer is the OR of its children's. A child written
/// before its parent (only a compaction re-anchor's copies do that) simply reads as
/// unanswered, which is the conservative side.
fn assistant_below(b: &Builder<'_>) -> Vec<bool> {
    let mut out = vec![false; b.len()];
    for i in (0..b.len()).rev() {
        out[i] = b
            .children_of(i)
            .iter()
            .any(|&c| out[c] || b.recs[c].r#type.as_deref() == Some("assistant"));
    }
    out
}

/// The surviving opener that replaced an abandoned one: the chain's own child of the
/// same parent, else the LAST non-abandoned opener sharing that `parentUuid`.
fn surviving_sibling(
    b: &Builder<'_>,
    _out: &Chain,
    last_opener: &HashMap<&str, usize>,
    i: usize,
) -> Option<usize> {
    let p = b.parent[i]?;
    if let Some(&c) = b.chain_child.get(p) {
        if c != i && b.opens[c] {
            return Some(c);
        }
    }
    last_opener.get(p).copied().filter(|&k| k != i)
}
