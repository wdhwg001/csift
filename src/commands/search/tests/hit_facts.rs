//! `unmodeled_shape`: which VALUE the 1:1 law names when csift models no leaf for a
//! line. The three axes it splits on, and the answers it must refuse to give.

use super::*;

/// A shape csift already models answers NOTHING here, on every axis. That is not a
/// formality: the caller reaches this helper only for a record classify left unlabeled,
/// so a helper that named a modeled value anyway would put `? unmodeled: system/api_error`
/// on a reader's screen for a leaf csift renders perfectly well - the one failure the
/// unit exists to prevent.
#[test]
fn a_modeled_shape_has_no_unmodeled_value_on_any_axis() {
    let modeled_subtype =
        rec(r#"{"type":"system","subtype":"api_error","uuid":"s1","content":"boom"}"#);
    assert_eq!(unmodeled_shape(&modeled_subtype, None), None);
    let modeled_payload = rec(
        r#"{"type":"attachment","uuid":"a1","attachment":{"type":"hook_additional_context","content":["x"]}}"#,
    );
    assert_eq!(unmodeled_shape(&modeled_payload, None), None);
    let modeled_line = rec(r#"{"type":"cost-state","sessionId":"s","totalCostUSD":1.0}"#);
    assert_eq!(unmodeled_shape(&modeled_line, None), None);
    // The two transcript types are never unmodeled either: their leaf is decided deeper.
    let user = rec(r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"hi"}}"#);
    assert_eq!(unmodeled_shape(&user, None), None);
}

/// And an UNmodeled shape names the value on the axis that decides it - the deeper one
/// wherever the top-level type is itself modeled, because the type is not what csift
/// failed to catch up with.
#[test]
fn an_unmodeled_shape_names_the_axis_that_decides_it() {
    let subtype = rec(r#"{"type":"system","subtype":"some_future_subtype","uuid":"s2"}"#);
    assert_eq!(
        unmodeled_shape(&subtype, None).as_deref(),
        Some("system/some_future_subtype")
    );
    let payload =
        rec(r#"{"type":"attachment","uuid":"a2","attachment":{"type":"some_future_payload"}}"#);
    assert_eq!(
        unmodeled_shape(&payload, None).as_deref(),
        Some("attachment/some_future_payload")
    );
    let line = rec(r#"{"type":"future-thing","uuid":"f1","note":"x"}"#);
    assert_eq!(
        unmodeled_shape(&line, None).as_deref(),
        Some("future-thing")
    );
    // A system line with no subtype at all, and a typeless line: no value to name.
    assert_eq!(
        unmodeled_shape(&rec(r#"{"type":"system","uuid":"s3"}"#), None),
        None
    );
    assert_eq!(unmodeled_shape(&rec(r#"{"uuid":"n1"}"#), None), None);
    // On a ROLE-bearing record the shape is the DISTINGUISHING block - the one that is
    // not the ordinary `text`. A record whose text block comes FIRST still names the
    // other one: `assistant/text` would tell a reader nothing about the line they
    // addressed, which is the whole reason the unit names a shape at all.
    let mixed = r#"{"type":"assistant","uuid":"m1","message":{"role":"assistant","content":[{"type":"text","text":"x"},{"type":"fallback","from":{"model":"a"},"to":{"model":"b"}}]}}"#;
    assert_eq!(
        unmodeled_shape(&rec(mixed), Some(mixed)).as_deref(),
        Some("assistant/fallback")
    );
}

// ── The ONE stamping site, read back from BOTH hit-construction sites ──
//
// Every real family ships here as a stub returning nothing, so what has to be pinned is
// not a family's answer but the wiring: that `family_facts` runs on a scanned unit AND on
// an addressed one, and that the per-unit `Section` reaches it with both halves filled.
// The test-only probe arm (`record_text::probe`) is what makes that observable.

use crate::commands::search::record_text::probe;

/// The marker heads on the units this record produced, in push order per unit.
fn marker_heads(hits: &[Hit]) -> Vec<Vec<&'static str>> {
    hits.iter()
        .map(|h| h.markers.iter().map(|m| m.head).collect())
        .collect()
}

/// Assert that the probe's own answer is PRESENT on this unit, exactly once - never that
/// it is the ONLY thing there.
///
/// The three seam tests below pin the WIRING, not a family's output, and every real
/// family is about to start stamping facts and markers of its own onto the very records
/// these fixtures use. An equality assertion would make each of those a failure here, in
/// a file the family does not own, for a reason that has nothing to do with the seam -
/// which is exactly what happened to the first family that landed a per-block fact. So:
/// contained, and contained once, because a probe arriving twice would mean the site is
/// called twice and that is a defect the equality form used to catch.
fn appears_once<T: PartialEq + std::fmt::Debug>(have: &[T], want: &T, what: &str) {
    let n = have.iter().filter(|x| *x == want).count();
    assert_eq!(n, 1, "{what}: expected {want:?} exactly once in {have:?}");
}

/// Each unit's FACTS as a consumer reads them: the label-zone tag the fact projects (`-`
/// when it projects none) and the `key=value` it appends to the hit's JSON object. Read
/// through [`HitFact::label_zone`]/[`HitFact::json`] rather than off the enum, because
/// those two are the whole surface a fact has - a variant that reached the hit and
/// projects nothing is not carried anywhere.
///
/// A fact that never arrived projects an EMPTY list, which is exactly what deleting the
/// fact half of `family_facts`' `merge`, or the `.chain(family)` in [`addressed_unit`],
/// produces. Markers keep arriving under both deletions, so only this reads them.
fn fact_projections(hits: &[Hit]) -> Vec<Vec<String>> {
    hits.iter()
        .map(|h| {
            h.facts
                .iter()
                .map(|f| {
                    let keys: Vec<String> = f
                        .json()
                        .into_iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect();
                    let zone = f.label_zone().unwrap_or_else(|| "-".to_string());
                    format!("{zone} | {}", keys.join(" "))
                })
                .collect()
        })
        .collect()
}

/// What the probe's own fact projects, built from the sentinel constant rather than from a
/// hit, so the assertion cannot pass by agreeing with whatever the code produced.
fn probe_fact() -> String {
    format!(
        "{} | family_probe={}",
        probe::SENTINEL_FACT,
        serde_json::json!(probe::SENTINEL_FACT)
    )
}

/// Emit every unit of one fixture record through the scan-side construction site.
fn scanned(line: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let record = rec(line);
    collect_record_hits(
        &record,
        Some(line),
        None,
        LabelFilter::all(),
        &Matcher::pure(),
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    hits
}

/// A SCANNED unit carries what its family said. This is the site the six family lanes
/// each cut for themselves; one cut, read back through the probe.
#[test]
fn the_scan_site_stamps_every_unit_a_record_produced() {
    let hits = scanned(
        r#"{"type":"assistant","uuid":"a1","sessionId":"family-facts-probe","message":{"role":"assistant","content":[{"type":"thinking","thinking":"t","signature":"s"},{"type":"text","text":"hello"}]}}"#,
    );
    // Two blocks, two units, and BOTH carry the probe's per-unit answer - a site that
    // stamped the record once would leave the second unit empty.
    let markers = marker_heads(&hits);
    assert_eq!(markers.len(), 2, "two blocks, two units");
    for (i, unit) in markers.iter().enumerate() {
        appears_once(unit, &probe::SENTINEL_MARKER, &format!("unit {i} marker"));
        appears_once(
            unit,
            &probe::SENTINEL_BLOCK_MARKER,
            &format!("unit {i} block marker"),
        );
    }
    // The FACT half of the seam arrives on both units too. It is a separate field merged
    // by a separate line, so the markers above say nothing about it.
    for (i, unit) in fact_projections(&hits).iter().enumerate() {
        appears_once(unit, &probe_fact(), &format!("unit {i} fact"));
    }
}

/// A RECORD-LEVEL unit reaches the same site, and its `Section` says it came from no
/// block - which is exactly the distinction a per-block family fact rests on.
#[test]
fn a_record_level_unit_is_stamped_without_a_block() {
    let hits = scanned(
        r#"{"type":"user","uuid":"u1","sessionId":"family-facts-probe","message":{"role":"user","content":"hello"}}"#,
    );
    let markers = marker_heads(&hits);
    assert_eq!(markers.len(), 1, "one record-level unit");
    appears_once(&markers[0], &probe::SENTINEL_MARKER, "marker");
    assert!(
        !markers[0].contains(&probe::SENTINEL_BLOCK_MARKER),
        "a record-level unit came from no block, and the probe says so"
    );
    appears_once(&fact_projections(&hits)[0], &probe_fact(), "fact");
}

/// And the ADDRESSED unit reaches it too. Without this call a `show --line N` render
/// would silently drop a fact the same record carries under `search`, which is the whole
/// reason the site is called at two places rather than one.
#[test]
fn the_address_site_stamps_the_unmodeled_unit() {
    let line = r#"{"type":"future-thing","uuid":"f1","sessionId":"family-facts-probe","note":"x"}"#;
    let hit = addressed_unit(
        &rec(line),
        Some(line),
        &ClassifyCtx::top_level(),
        &Matcher::pure(),
        EXCERPT_MAX,
    )
    .expect("a parseable line always renders one unit");
    let heads: Vec<&str> = hit.markers.iter().map(|m| m.head).collect();
    appears_once(&heads, &probe::SENTINEL_MARKER, "marker");
    appears_once(&heads, &probe::SENTINEL_LEAFLESS_MARKER, "leafless marker");
    // And so does the FACT - beside the SHAPE fact, which the label still finds wherever
    // the list grows. This is the assertion the `.chain(family)` line answers to: drop it
    // and the markers above still arrive while the family's facts vanish.
    let facts = &fact_projections(std::slice::from_ref(&hit))[0];
    appears_once(facts, &probe_fact(), "fact");
    appears_once(
        facts,
        &"- | unmodeled=\"future-thing\"".to_string(),
        "shape fact",
    );
    // And the label ZONE carries them too. The shape replaces the leaf path; everything
    // else a family attached renders after it, in the same push order a labelled unit
    // uses. A leafless unit whose facts reached JSON alone was the one place the two
    // formats disagreed about the same record.
    assert_eq!(
        render_label(&hit),
        "unmodeled: future-thing [family probe] [family probe leafless] family probe fact"
    );
}

/// A record no family owns costs an empty answer, never a fabricated one: the probe is
/// keyed on a sentinel and every real family stub returns nothing, so an ordinary record
/// leaves both extension points empty.
#[test]
fn a_record_no_family_owns_carries_neither_extension_point() {
    let hits = scanned(
        r#"{"type":"user","uuid":"u2","sessionId":"0a1b2c3d-0000-0000-0000-000000000000","message":{"role":"user","content":"hello"}}"#,
    );
    assert_eq!(hits.len(), 1);
    assert!(hits[0].markers.is_empty());
    assert!(hits[0].facts.is_empty());
}

/// A `headers` member is elided at EVERY depth, and the elision is keyed on the NAME.
///
/// The key=value excerpt is the one rendered path that walks a line's own values without
/// knowing what they are, so it is the one path an HTTP response header set can reach -
/// and the measured set carries a session cookie and an organization id. The unit reads
/// the rule at each depth the shape can put it: the top level, inside an object, and
/// inside an array of objects, because a rule that only covered `error.headers` would be
/// a rule about one build's shape rather than about header sets.
#[test]
fn a_headers_member_is_elided_at_every_depth() {
    let excerpt = |line: &str| {
        addressed_unit(
            &rec(line),
            Some(line),
            &ClassifyCtx::top_level(),
            &Matcher::pure(),
            EXCERPT_MAX,
        )
        .expect("a parseable line renders one unit")
        .excerpt
    };

    // Nested one deep, the api_error shape.
    let nested = r#"{"type":"zz-unmodeled","uuid":"h1","error":{"type":"overloaded_error","headers":{"set-cookie":"zzdonotprint"}}}"#;
    let out = excerpt(nested);
    assert!(
        !out.contains("zzdonotprint"),
        "a nested value leaked: {out}"
    );
    assert!(out.contains("<elided>"), "and the elision is stated: {out}");
    assert!(
        out.contains("overloaded_error"),
        "the object's other fields still render: {out}"
    );

    // At the TOP level, where the value is the header object itself.
    let top = r#"{"type":"zz-unmodeled","uuid":"h2","headers":{"set-cookie":"zzdonotprint"}}"#;
    let out = excerpt(top);
    assert!(
        !out.contains("zzdonotprint"),
        "a top-level value leaked: {out}"
    );
    assert!(out.contains("headers=<elided>"), "{out}");

    // Inside an ARRAY of objects - two containers down, which a shape with a retry log
    // would produce.
    let deep = r#"{"type":"zz-unmodeled","uuid":"h3","attempts":[{"n":1,"headers":{"set-cookie":"zzdonotprint"}}]}"#;
    let out = excerpt(deep);
    assert!(
        !out.contains("zzdonotprint"),
        "an array value leaked: {out}"
    );
    assert!(out.contains("<elided>"), "{out}");

    // A key that merely CONTAINS the word is not the header set and is untouched: the
    // rule is an exact name, so it can neither over-elide nor be evaded by nesting.
    let other = r#"{"type":"zz-unmodeled","uuid":"h4","headersSeen":3,"note":"zzkeepme"}"#;
    let out = excerpt(other);
    assert!(out.contains("zzkeepme"), "{out}");
    assert!(out.contains("headersSeen=3"), "{out}");
}

/// A `data` member is elided the same way, and for a different reason with the same
/// answer: not secrecy, but the per-value CUT.
///
/// Two on-disk shapes carry one and both are opaque blobs - a `{type:"base64"}` image or
/// document source, and a `redacted_thinking` block's ciphertext. Their head identifies
/// nothing, and the cut is 80 characters, so the blob swallows the whole budget and the
/// sibling keys that DO identify the shape never print. This asserts exactly that trade:
/// with the blob elided, `media_type` reaches the excerpt.
#[test]
fn a_data_blob_is_elided_so_the_keys_that_identify_the_shape_can_print() {
    let excerpt = |line: &str| {
        addressed_unit(
            &rec(line),
            Some(line),
            &ClassifyCtx::top_level(),
            &Matcher::pure(),
            EXCERPT_MAX,
        )
        .expect("a parseable line renders one unit")
        .excerpt
    };
    // An image-only NON-isMeta user record: the one image shape csift models no leaf for.
    // The payload is longer than the 80-character per-value cut, which is the whole point.
    let payload = "A".repeat(200);
    let line = format!(
        r#"{{"type":"user","uuid":"i1","message":{{"role":"user","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{payload}"}}}}]}}}}"#
    );
    let out = excerpt(&line);
    assert!(
        !out.contains("AAAAAAAAAA"),
        "the blob's head is noise and must not print: {out}"
    );
    assert!(out.contains(r#""data":"<elided>""#), "{out}");
    assert!(
        out.contains("image/png"),
        "and the key that identifies the shape now fits in the cut: {out}"
    );

    // A SHORT `data` member goes too - the rule is the name, not the length, so it can
    // neither be evaded by a small blob nor need a threshold nobody measured.
    let short = r#"{"type":"zz-unmodeled","uuid":"i2","data":"zzblob","note":"zzkeepme"}"#;
    let out = excerpt(short);
    assert!(
        out.contains("data=<elided>") && out.contains("zzkeepme"),
        "{out}"
    );
}

/// The replay pointer renders the SURVIVOR's physical line, and a jsonl line number is
/// 1-based - so a map answering ZERO is answering "I have no line", not "line 0". The
/// guard that drops it is the difference between `replay copy of L42` and a pointer at a
/// line no address can ever fetch.
#[test]
fn a_replay_pointer_at_a_line_that_cannot_exist_is_dropped() {
    // A re-anchored block lands twice with its uuids preserved, so the EARLIER line is
    // the copy and the later one is the survivor (the loader's map keeps the last).
    let lines = [
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first"}}"#,
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"first"}}"#,
    ];
    let records: Vec<Record> = lines.iter().map(|l| rec(l)).collect();
    let chain = crate::model::Chain::build(&records, None);
    assert_eq!(chain.replay_of(0), Some(1), "the earlier line is the copy");

    let mut hits = scanned(lines[0]);
    let real: HashMap<usize, usize> = [(1usize, 42usize)].into_iter().collect();
    backfill_survival(&mut hits, &chain, 0, &real, None);
    assert_eq!(hits[0].replay_copy_of, Some(42));

    let mut hits = scanned(lines[0]);
    let impossible: HashMap<usize, usize> = [(1usize, 0usize)].into_iter().collect();
    backfill_survival(&mut hits, &chain, 0, &impossible, None);
    assert_eq!(
        hits[0].replay_copy_of, None,
        "L0 addresses nothing - the pointer is dropped, never rendered"
    );
}

/// One transcript's rows, from its lines, at their own 1-based line numbers.
fn rows_of(lines: &[&str]) -> Vec<Kept> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| Kept {
            rec: rec(l),
            can_hit: true,
            line_no: i + 1,
            from_sidecar: false,
            gated_raw: None,
        })
        .collect()
}

/// The FOUR survival relations, computed directly from the chain rather than read out of a
/// rendered hit.
///
/// One fixture carries all four, because they are four readings of ONE walk and a fixture
/// per relation would let three of them pass while the walk that feeds them moved. L1 and
/// L2 share a uuid, so L1 is a replay copy of L2 (`replays`, basis KEY - the shared uuid is
/// what the loader itself joins on). L4 is a prompt nothing answered, L5 one that WAS
/// answered and then rewound past, and L7 the surviving opener that replaced both (all
/// three share the parent L3) - so L4 `supersedes` and L5 `continues` name L7, while L6
/// hangs off L5 on that abandoned branch and `branches` names its head.
#[test]
fn the_survival_relations_name_the_records_the_chain_found() {
    let kept = rows_of(&[
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#,
        r#"{"type":"user","uuid":"d1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the chanel"}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"s1","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#,
        r#"{"type":"user","uuid":"s2","parentUuid":"a0","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"survey the shoal instead"}}"#,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"s2","timestamp":"2026-06-07T05:03:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#,
    ]);
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let chain = crate::model::Chain::build_by(&rows, |r| r.node(), None);

    // `(word, basis, target line)` for one record's edges - which relation, how it was
    // found, and the address it resolves to, which is the whole content of the fact.
    let at = |i: usize| -> Vec<(&'static str, &'static str, Option<usize>)> {
        survival_edges(&chain, i, &rows)
            .iter()
            .map(|f| match f {
                HitFact::For {
                    kind,
                    basis,
                    target,
                } => (kind.as_str(), basis.as_str(), target.line),
                other => panic!("a survival relation resolved to nothing: {other:?}"),
            })
            .collect()
    };
    assert_eq!(at(0), [("replays", "key", Some(2))]);
    assert_eq!(at(3), [("supersedes", "position", Some(7))]);
    assert_eq!(at(4), [("continues", "position", Some(7))]);
    assert_eq!(at(5), [("branches", "position", Some(5))]);
    // The branch HEAD names no branch: an edge from a record to itself points at nothing,
    // and the head's own `user.rewound` leaf already says what it is.
    assert!(
        !at(4).iter().any(|(k, _, _)| *k == "branches"),
        "the head of a branch carries no edge to itself"
    );
    // A record on the surviving conversation carries none of the four.
    assert_eq!(at(7), []);
}

/// A relation whose target row is absent answers nothing rather than pointing somewhere.
///
/// `survival_edges` resolves a chain INDEX against the row list, and the two always come
/// from one scan - but this helper is where they could disagree, so the guard is pinned
/// rather than assumed. A fabricated line here would be an address that fetches the wrong
/// record, which is worse than no edge at all.
#[test]
fn a_survival_relation_whose_row_is_out_of_range_yields_no_edge() {
    let kept = rows_of(&[
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
    ]);
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let chain = crate::model::Chain::build_by(&rows, |r| r.node(), None);
    assert_eq!(chain.replay_of(0), Some(1), "the earlier line is the copy");
    assert!(
        survival_edges(&chain, 0, &rows[..1]).is_empty(),
        "the survivor is not in the rows handed in, so there is no address to name"
    );
}

/// A survival relation whose target has NO physical line renders its uuid, never `L0`.
///
/// The one record csift holds with no line is a merged elicitation-sidecar record, which
/// carries `line_no: 0` because there is no jsonl line to name. `L0` addresses nothing -
/// `csift show --line 0` fetches no record - so an edge that printed it would hand a reader
/// a command that cannot work, in place of the id that at least identifies the target.
///
/// Asserted on the EDGE rather than on the guard, because the guard is one comparison and
/// what matters is the wire shape it produces: a target with a uuid, no line, and the
/// builder's own `not in this file` note.
#[test]
fn a_survival_target_with_no_physical_line_names_its_uuid() {
    let lines = [
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
        r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
    ];
    // The SURVIVOR carries no line, which is the shape the guard is for.
    let kept: Vec<Kept> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| Kept {
            rec: rec(l),
            can_hit: true,
            line_no: if i == 1 { 0 } else { 1 },
            from_sidecar: i == 1,
            gated_raw: None,
        })
        .collect();
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let chain = crate::model::Chain::build_by(&rows, |r| r.node(), None);
    assert_eq!(chain.replay_of(0), Some(1), "the earlier line is the copy");
    let facts = survival_edges(&chain, 0, &rows);
    let HitFact::For { kind, target, .. } = &facts[0] else {
        panic!("a resolved edge: {facts:?}");
    };
    assert_eq!(kind.as_str(), "replays");
    assert_eq!(
        target.line, None,
        "L0 addresses nothing, so no line is named"
    );
    assert_eq!(target.uuid.as_deref(), Some("u0"));
    assert_eq!(target.note.as_deref(), Some("not in this file"));
}
