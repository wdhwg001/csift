//! Per-record FACTS stamped onto a hit that is already built: the unlabeled-unit
//! fallback an address renders, the compaction pair's own fields, the survival answer
//! and the `show --line`/`--uuid` address. Split out of `hits.rs` (the emission engine)
//! at the 600-line structure gate; every one of these runs per RECORD, after its label
//! set is known, so none of them may re-derive what `classify` already decided.

use super::*;

/// The ONE unit an ADDRESSED record with no modeled leaf renders (see the refetch-law note in
/// [`collect_turn_hits`]): the record's own raw text under an EMPTY label ([`Hit::class`] is
/// `None`), so a reader sees the bytes csift has rather than a bail. `None` when the record
/// carries no text anywhere - there is nothing to render and the address stays a miss, which is
/// the honest answer for a session-state cache line. The address/line/uuid are backfilled by the
/// caller like any other hit.
pub(crate) fn unlabeled_hit(rec: &Record, matcher: &Matcher, excerpt_max: usize) -> Option<Hit> {
    let text = record_raw_text(rec)?;
    let span = matcher.locate(&text)?;
    let (excerpt, truncated) = match_excerpt(&text, span, excerpt_max);
    Some(Hit {
        class: None,
        labels: Vec::new(),
        excerpt,
        timestamp_utc: rec.timestamp.clone(),
        tool_name: None,
        model: None,
        attachment_type: rec.attachment_type(),
        version: rec.version.clone(),
        is_error: None,
        direction: None,
        tool_use_id: None,
        pair: None,
        line: 0,
        uuid: None,
        raw: None,
        image_ids: Vec::new(),
        from_sidecar: false,
        queue_operation: None,
        queue_reason: None,
        task_ids: Vec::new(),
        orphan_kind: None,
        delivery: rec.delivery_override(),
        // An unlabeled unit is by definition a record csift models no leaf for, so it is
        // never a resume placeholder (that shape has one).
        resume_paired: None,
        survival: crate::model::Survival::Live,
        rewound_branch: false,
        replay_copy_of: None,
        compaction: None,
        truncated,
    })
}

/// The C-33 [`CompactionHit`] for a compaction BOUNDARY or SUMMARY record; `None` for anything
/// else. The boundary hands over its `compactMetadata` verbatim (JSON keeps the full uuid lists
/// the one-line excerpt only counts) plus the mode the per-file [`crate::model::SummarizeIndex`]
/// paired to it; the summary hands over its own mode and the verbatim `direction`. A boundary the
/// scan never paired keeps a null mode - an unpaired boundary is exactly the shape a windowed read
/// produces, so guessing `compact` there would invent a fact.
///
/// PERF (SPEC section 7): the caller gates this on the record's ALREADY-COMPUTED label set, so
/// the common record never reaches the body at all - never re-read record fields here to decide
/// whether a record is a compaction one.
pub(crate) fn compaction_facts(rec: &Record, ctx: &ClassifyCtx) -> Option<Box<CompactionHit>> {
    if rec.is_compact_boundary() {
        return Some(Box::new(CompactionHit {
            metadata: rec.compact_metadata.clone(),
            mode: ctx
                .summarize
                .and_then(|ix| ix.mode_for_boundary(rec.uuid.as_deref())),
            direction: None,
        }));
    }
    if rec.is_compact_summary.unwrap_or(false) {
        return Some(Box::new(CompactionHit {
            metadata: None,
            mode: rec.summary_compaction_mode(),
            direction: rec.summarize_direction().map(str::to_string),
        }));
    }
    None
}

/// Stamp the SURVIVAL AXIS answer onto each hit just appended: is this record still in
/// the conversation, is its branch a rewound one, and is the line an earlier copy of a
/// record a later line carries. The replay pointer is rendered as the SURVIVOR's physical
/// line, which is only resolvable here where the record list is in hand.
pub(crate) fn backfill_survival(
    hits: &mut [Hit],
    chain: &crate::model::Chain,
    idx: usize,
    survivor_line: &HashMap<usize, usize>,
) {
    let survival = chain.survival(idx);
    let rewound = chain.on_rewound_branch(idx);
    let replay = chain
        .replay_of(idx)
        .and_then(|s| survivor_line.get(&s).copied())
        .filter(|&l| l > 0);
    for h in hits.iter_mut() {
        h.survival = survival;
        h.rewound_branch = rewound;
        h.replay_copy_of = replay;
    }
}

/// Stamp the source record's line number + uuid onto each hit just appended for it - the
/// `csift show --line/--uuid` address. Done by the turn collector (not `make_hit`) because the line number
/// lives on the `Kept`, not the `Record`. Also attaches the record's image ids to its FIRST
/// hit (so an image-bearing message exposes the extractable `#N`/`L<line>i<n>` id once, not
/// repeated per matched block).
pub(crate) fn backfill_address(hits: &mut [Hit], kept: &Kept) {
    for h in hits.iter_mut() {
        h.line = kept.line_no;
        h.uuid = kept.rec.uuid.clone();
        h.from_sidecar = kept.from_sidecar;
    }
    if let Some(first) = hits.first_mut() {
        first.image_ids = crate::image::image_ids_for_record(&kept.rec, kept.line_no);
    }
}
