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
                    let (key, value) = f.json();
                    let zone = f.label_zone().unwrap_or_else(|| "-".to_string());
                    format!("{zone} | {key}={value}")
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
    assert_eq!(
        marker_heads(&hits),
        vec![
            vec![probe::SENTINEL_MARKER, probe::SENTINEL_BLOCK_MARKER],
            vec![probe::SENTINEL_MARKER, probe::SENTINEL_BLOCK_MARKER],
        ]
    );
    // The FACT half of the seam arrives on both units too. It is a separate field merged
    // by a separate line, so the markers above say nothing about it.
    assert_eq!(
        fact_projections(&hits),
        vec![vec![probe_fact()], vec![probe_fact()]]
    );
}

/// A RECORD-LEVEL unit reaches the same site, and its `Section` says it came from no
/// block - which is exactly the distinction a per-block family fact rests on.
#[test]
fn a_record_level_unit_is_stamped_without_a_block() {
    let hits = scanned(
        r#"{"type":"user","uuid":"u1","sessionId":"family-facts-probe","message":{"role":"user","content":"hello"}}"#,
    );
    assert_eq!(marker_heads(&hits), vec![vec![probe::SENTINEL_MARKER]]);
    assert_eq!(fact_projections(&hits), vec![vec![probe_fact()]]);
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
    assert_eq!(
        hit.markers.iter().map(|m| m.head).collect::<Vec<_>>(),
        vec![probe::SENTINEL_MARKER, probe::SENTINEL_LEAFLESS_MARKER]
    );
    // And so does the FACT - behind the shape fact, which still leads so `render_label`
    // reads it as the label. This is the assertion the `.chain(family)` line answers to:
    // drop it and the markers above still arrive while the family's facts vanish.
    assert_eq!(
        fact_projections(std::slice::from_ref(&hit)),
        vec![vec![
            "- | unmodeled=\"future-thing\"".to_string(),
            probe_fact()
        ]]
    );
    assert_eq!(render_label(&hit), "unmodeled: future-thing");
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
