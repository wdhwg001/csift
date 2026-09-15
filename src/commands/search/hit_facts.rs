//! Per-record FACTS stamped onto a hit that is already built: the unlabeled-unit
//! fallback an address renders, the compaction pair's own fields, the survival answer
//! and the `show --line`/`--uuid` address. Split out of `hits.rs` (the emission engine)
//! at the 600-line structure gate; every one of these runs per RECORD, after its label
//! set is known, so none of them may re-derive what `classify` already decided.

use super::*;

/// The ONE unit an ADDRESSED record with no modeled leaf renders (see the refetch-law note in
/// [`collect_turn_hits`]): the record under an EMPTY label ([`Hit::class`] is `None`), so a
/// reader sees what csift has rather than a bail.
///
/// The 1:1 law says an address never hard-misses a PARSEABLE line, so the unit has two
/// bodies. A record with text renders that text, exactly as before. A record with NONE -
/// a line type csift models no leaf for, a subtype a later build added - renders a
/// key=value excerpt of its own top-level keys instead, and the unit NAMES its shape
/// ([`HitFact::Unmodeled`]) so the reader learns what the line IS. The excerpt is
/// fabricated, which is safe here and only here: an address needs no match, so no
/// prefilter or whole-file gate is reasoning about this text.
///
/// `None` only when the line is no record at all (no text, no keys) - the address then
/// stays a miss, which is the honest answer.
pub(crate) fn unlabeled_hit(
    rec: &Record,
    raw: Option<&str>,
    matcher: &Matcher,
    excerpt_max: usize,
) -> Option<Hit> {
    let shape = unmodeled_shape(rec);
    let text = match record_raw_text(rec, raw) {
        Some(t) => t,
        None => top_level_key_excerpt(raw)?,
    };
    let span = matcher.locate(&text)?;
    let (excerpt, truncated) = match_excerpt(&text, span, excerpt_max);
    Some(Hit {
        class: None,
        labels: Vec::new(),
        excerpt,
        body: uncapped_body(&text, excerpt_max),
        timestamp_utc: rec.timestamp.clone(),
        tool_name: None,
        model: None,
        attachment_type: rec.attachment_type(),
        version: rec.version.clone(),
        is_error: None,
        denial_kind: rec.tool_denial_kind.clone(),
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
        // An unlabeled unit carries no leaf, so it is never the fired prompt.
        scheduled_at: None,
        truncated,
        markers: Vec::new(),
        // The shape csift models no leaf for. An address is the only way here, and
        // naming the shape is the whole point of the unit: a reader learns WHAT the
        // line is rather than that csift had nothing to say.
        facts: shape.into_iter().map(HitFact::Unmodeled).collect(),
    })
}

/// The on-disk SHAPE of a record csift models no leaf for: its top-level `type`, and
/// the deeper axis where the shape splits on one - `system/<subtype>`,
/// `attachment/<payload type>`. `None` when the record HAS a leaf somewhere (the caller
/// only reaches this for a record that produced no unit, so that means a text-less
/// block record rather than an unknown shape) or when the line carries no `type` at all.
///
/// Naming the shape is what makes the unit useful: `? unmodeled: system/some_new_subtype`
/// tells a reader exactly which build-side shape csift has not caught up with, which
/// `? (no label)` never did.
pub(crate) fn unmodeled_shape(rec: &Record) -> Option<String> {
    let ty = rec.r#type.as_deref()?;
    // A record whose type IS modeled but whose own leaf is decided deeper: name the
    // deeper value, which is the part csift did not model.
    if ty == "system" {
        return Some(match rec.subtype.as_deref() {
            Some(s) if crate::model::system_leaf(s).is_none() => format!("system/{s}"),
            Some(_) | None => return None,
        });
    }
    if ty == "attachment" {
        return match rec.attachment_type() {
            Some(a) if crate::model::attachment_leaf(&a).is_none() => {
                Some(format!("attachment/{a}"))
            }
            _ => None,
        };
    }
    // Any other top-level type with no leaf of its own.
    (crate::model::bookkeeping_leaf(ty).is_none() && ty != "user" && ty != "assistant")
        .then(|| ty.to_string())
}

/// A key=value excerpt of a line's own top-level keys, for a record that carries no
/// renderable text at all. Keys in their on-disk order, values cut at 80 characters, so
/// a reader sees the SHAPE of an unknown line without csift claiming to interpret it.
/// `None` when the caller has no source line (every ordinary record) or the line is not
/// a JSON object with keys.
fn top_level_key_excerpt(raw: Option<&str>) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw?).ok()?;
    let obj = v.as_object()?;
    let parts: Vec<String> = obj
        .iter()
        .map(|(k, v)| {
            let rendered = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{k}={}", crate::text::truncate_excerpt(&rendered, 80))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
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
        first.image_ids = crate::commands::image::image_ids_for_record(&kept.rec, kept.line_no);
    }
}
