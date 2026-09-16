//! Per-record FACTS stamped onto a hit that is already built: the unlabeled-unit
//! fallback an address renders, the compaction pair's own fields, the survival answer
//! and the `show --line`/`--uuid` address. Split out of `hits.rs` (the emission engine)
//! at the 600-line structure gate; every one of these runs per RECORD, after its label
//! set is known, so none of them may re-derive what `classify` already decided.

use super::*;

/// The ONE STAMPING SITE: everything the six taxonomy families have to say about the
/// unit being emitted, gathered in a fixed order.
///
/// A family owns one classify file and one render file and nothing else, so it needs a
/// seam in the emission engine to hang a [`Marker`] or a [`HitFact`] on. Without a shared
/// seam each family cuts its own, in the same three shared files, and the six cuts do not
/// compose. This is that seam: `hits.rs` calls it once per emitted unit and
/// [`addressed_unit`] calls it once for the unit an address renders, so a record fetched
/// by `show --line` carries exactly what the same record carries under `search`.
///
/// Order is fixed and is the order of the map's own families. It matters only for the
/// label zone, where markers render in push order.
///
/// COST on a unit no family owns: six calls that each compare one field and return two
/// `Vec::new()`, which allocate nothing, plus two extends of nothing. The gate belongs
/// inside each family (it knows what its own records look like); this function does not
/// pre-filter, because a pre-filter here would be a seventh place that has to know every
/// family's shape.
pub(crate) fn family_facts(
    rec: &Record,
    raw: Option<&str>,
    section: &Section<'_>,
    ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    fn merge(out: &mut (Vec<HitFact>, Vec<Marker>), add: (Vec<HitFact>, Vec<Marker>)) {
        out.0.extend(add.0);
        out.1.extend(add.1);
    }
    let mut out: (Vec<HitFact>, Vec<Marker>) = (Vec::new(), Vec::new());
    merge(
        &mut out,
        record_text::bookkeeping::facts(rec, raw, section, ctx),
    );
    merge(
        &mut out,
        record_text::attachments::facts(rec, raw, section, ctx),
    );
    merge(
        &mut out,
        record_text::injections::facts(rec, raw, section, ctx),
    );
    merge(&mut out, record_text::system::facts(rec, raw, section, ctx));
    merge(
        &mut out,
        record_text::thinking::facts(rec, raw, section, ctx),
    );
    merge(&mut out, record_text::keys::facts(rec, raw, section, ctx));
    #[cfg(test)]
    merge(&mut out, record_text::probe::facts(rec, raw, section, ctx));
    out
}

/// The ONE unit an ADDRESSED record renders when the ordinary passes produced nothing
/// (see the refetch-law note in [`collect_turn_hits`]) - the 1:1 law's whole mechanism.
///
/// TWO records reach it, and they are not the same record. One carries NO leaf: an isMeta
/// template csift models none of, a block shape it has never seen, a line type a later
/// build added. It renders under an EMPTY label with its shape NAMED
/// ([`HitFact::Unmodeled`]), so the reader learns what the line is instead of that csift
/// had nothing to say. The other carries a leaf and simply has no TEXT - a `fallback`
/// block, an image with no caption - and it keeps its leaf; losing the label there would
/// be the mislabeling the unit exists to avoid, in the other direction.
///
/// Either way the BODY falls back to a key=value excerpt of the line's own top-level keys
/// when the record renders no text. That excerpt is fabricated, which is safe here and
/// only here: an address needs no match, so no prefilter and no whole-file gate is
/// reasoning about this text.
///
/// `None` only when the line is no record at all (no text, no keys) - the address then
/// stays a miss, which is the honest answer.
pub(crate) fn addressed_unit(
    rec: &Record,
    raw: Option<&str>,
    ctx: &ClassifyCtx,
    matcher: &Matcher,
    excerpt_max: usize,
) -> Option<Hit> {
    let labels = rec.classify(ctx);
    let class = labels.first().copied();
    let shape = if class.is_none() {
        unmodeled_shape(rec, raw)
    } else {
        None
    };
    let text = match record_raw_text(rec, raw, class, ctx) {
        Some(t) => t,
        None => top_level_key_excerpt(raw)?,
    };
    let span = matcher.locate(&text)?;
    let (excerpt, truncated) = match_excerpt(&text, span, excerpt_max);
    // The SECOND call of the one stamping site. An address renders the record it names,
    // so it has to render what a scan would have rendered: a family fact that only ever
    // reached a scanned unit would make `show --line N` disagree with the `search` hit
    // it was copied from. The unit carries no block, and it carries the leaf when it has
    // one (this unit is emitted both for a record with no leaf and for one whose leaf
    // simply has no text).
    let (family, markers) = family_facts(rec, raw, &Section::record(class), ctx);
    Some(Hit {
        class,
        labels: labels.iter().map(|c| c.path()).collect(),
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
        markers,
        // The shape csift models no leaf for. An address is the only way here, and
        // naming the shape is the whole point of the unit: a reader learns WHAT the
        // line is rather than that csift had nothing to say. The family facts follow it,
        // so `render_label` still finds the shape wherever the list grows.
        facts: shape
            .into_iter()
            .map(HitFact::Unmodeled)
            .chain(family)
            .collect(),
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
pub(crate) fn unmodeled_shape(rec: &Record, raw: Option<&str>) -> Option<String> {
    let ty = rec.r#type.as_deref()?;
    // A ROLE-BEARING record with no leaf. Its type is modeled and says nothing, so the
    // shape is the BLOCK it carries - the axis its leaf would have been decided on -
    // and, when it carries no distinguishing block, the authorship flag that makes it
    // the harness's own text rather than the operator's.
    if ty == "user" || ty == "assistant" {
        if let Some(b) = distinguishing_block_type(raw) {
            return Some(format!("{ty}/{b}"));
        }
        return rec.is_meta.unwrap_or(false).then(|| format!("{ty}/isMeta"));
    }
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
    (crate::model::bookkeeping_leaf(ty).is_none()).then(|| ty.to_string())
}

/// The `type` of the first content block that is not `text`, read off the RAW line rather
/// than the parsed record: the block model folds every shape it does not know into one
/// tolerant arm, so the parsed form cannot tell a `document` from a `fallback`, and the
/// whole point of the unit is to name the value csift has not caught up with. `None` when
/// there is no raw line, no block array, or nothing but text blocks - a record of text
/// alone already had its leaf decided elsewhere.
fn distinguishing_block_type(raw: Option<&str>) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw?).ok()?;
    let types: Vec<&str> = v
        .get("message")?
        .get("content")?
        .as_array()?
        .iter()
        .filter_map(|b| b.get("type")?.as_str())
        .collect();
    // The DISTINGUISHING type is the one that is not the ordinary `text`. When every
    // block IS text - a whitespace-only assistant turn - `text` IS the answer: the unit
    // reads `? unmodeled: assistant/text` rather than a bare `? unmodeled` with no shape
    // at all, which tells a reader nothing about the line they addressed.
    types
        .iter()
        .find(|t| **t != "text")
        .or_else(|| types.first())
        .map(|t| (*t).to_string())
}

/// What a `headers` member renders as, everywhere and always: nothing.
pub(crate) const HEADERS_ELIDED: &str = "<elided>";

/// Replace every `headers` member of a value, at any depth, with [`HEADERS_ELIDED`].
///
/// An HTTP RESPONSE HEADER SET is the one thing a rendered transcript view must not
/// carry. A `type:"system"` record of the `api_error` shape holds the whole set inside
/// its `error` object, and the measured set carries a session cookie, an organization id
/// and two request trace ids; csift output is pasted into reports, into chats and into
/// other models' contexts, so a header set that reaches a rendered surface has left the
/// machine it was on. `--raw` prints the source line and says so, which is what an escape
/// hatch is for.
///
/// This is the UNMODELED unit's own guard, and it is deliberately blunt: the excerpt is a
/// shape preview for a line csift models nothing about, so it cannot know which `headers`
/// is the dangerous one and elides every one of them. A family that renders a known shape
/// names the fields it wants instead, which is the same law from the other side.
fn elide_headers(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, val)| {
                    let val = if k == "headers" {
                        serde_json::json!(HEADERS_ELIDED)
                    } else {
                        elide_headers(val)
                    };
                    (k.clone(), val)
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(elide_headers).collect())
        }
        other => other.clone(),
    }
}

/// A key=value excerpt of a line's own top-level keys, for a record that carries no
/// renderable text at all. Keys in their on-disk order, values cut at 80 characters, so
/// a reader sees the SHAPE of an unknown line without csift claiming to interpret it.
/// A `headers` member is elided at every depth ([`elide_headers`]).
/// `None` when the caller has no source line (every ordinary record) or the line is not
/// a JSON object with keys.
fn top_level_key_excerpt(raw: Option<&str>) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw?).ok()?;
    let obj = v.as_object()?;
    let parts: Vec<String> = obj
        .iter()
        .map(|(k, v)| {
            let rendered = match v {
                _ if k == "headers" => HEADERS_ELIDED.to_string(),
                serde_json::Value::String(s) => s.clone(),
                other => elide_headers(other).to_string(),
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
