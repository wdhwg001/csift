//! One record, one hit: what a flagless scan EMITS for a shape, as against which leaf it
//! carries. The two can disagree in both directions - a record emitting twice because two
//! lanes each answered for it, and a record emitting nothing because its leaf's only
//! render path was one the record has no input for - and each direction cost a release
//! defect, so the law has its own file and its own sweep.

use super::*;

/// J2/J7: one record, one hit. The taxonomy's own table (`model::inventory::tests::
/// the_image_leaf_follows_the_shape_not_the_block`) asserts the LABELS for these shapes;
/// this asserts what the emission does with them, because the two can disagree: the
/// text+image assistant turn carried one leaf and emitted TWO hits, the record-text lane
/// and the per-block lane each answering for the same record, with the harness view
/// printed first while `labels[]` ordered richest-first put the message first.
///
/// The filter is wide open, which is what a flagless scan runs, and the pattern matches
/// every body, so a second lane cannot hide behind a narrow match.
#[test]
fn one_record_emits_one_hit_for_every_image_shape() {
    // Shape, line, expected hit count. Kept beside the taxonomy table deliberately: that
    // one is about leaves, this one about emission, and a fixture shared across two
    // #[cfg(test)] modules would couple test files the layout keeps independent.
    const SHAPES: &[(&str, &str, usize)] = &[
        // ZERO, and not because the leaf is missing: this record CARRIES
        // `harness.context.image` (the taxonomy table asserts it) and renders in full
        // under `show --line`, with its leaf, a key=value body and the `csift image`
        // pointer. A pattern SCAN emits nothing because the record has no text SECTION to
        // match or excerpt - the ordinary textless-record rule - and `csift image` is the
        // surface that reads what it does carry.
        (
            "isMeta image-only user",
            r#"{"type":"user","uuid":"e1","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"zzimg"}}]},"isMeta":true}"#,
            0,
        ),
        (
            "non-isMeta user text+image",
            r#"{"type":"user","uuid":"e2","message":{"role":"user","content":[{"type":"text","text":"zzbody what do you make of this"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"zzimg"}}]}}"#,
            1,
        ),
        (
            "assistant text+image",
            r#"{"type":"assistant","uuid":"e3","message":{"role":"assistant","content":[{"type":"text","text":"zzbody here is the chart"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"zzimg"}}]}}"#,
            1,
        ),
        (
            "assistant image-only",
            r#"{"type":"assistant","uuid":"e4","message":{"role":"assistant","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"zzimg"}}]}}"#,
            0,
        ),
        (
            "assistant document-only",
            r#"{"type":"assistant","uuid":"e5","message":{"role":"assistant","content":[{"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"zzimg"}}]}}"#,
            0,
        ),
    ];

    for (name, line, want) in SHAPES {
        let r = rec(line);
        let m = build_matcher(&args("")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(
            &r,
            None,
            None,
            LabelFilter::all(),
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(
            hits.len(),
            *want,
            "{name} should emit {want} hit(s), got {:?}",
            hits.iter().map(|h| h.class).collect::<Vec<_>>()
        );
    }
}

/// K2: every VISIBLE leaf a specimen carries is one a FLAGLESS scan can emit - exactly
/// once. Twice now a record has carried a leaf that only `show --line` could reach: the
/// human's queued steer (the render resolved through an opener path an attachment has
/// none of) and the coordinator relay (the same path, on an isMeta record that opens no
/// turn). Both read zero in a census and nothing under their own `-t`, so the label was a
/// statement nobody could check.
///
/// The specimens are the taxonomy's own (`model::specimens`), so this sweep
/// and the drift tests answer different questions about the SAME records: which leaf, and
/// whether one record emits one hit.
///
/// A record carrying NO leaf emits nothing, which is the unmodeled unit's territory and
/// belongs to an address. A record whose only leaves are INVISIBLE is gated away from a
/// flagless scan by construction, so it is not swept here.
#[test]
fn every_visible_leaf_specimen_emits_exactly_one_hit() {
    // The two SHAPES that carry a visible leaf and have no text section to emit. Keyed on
    // the specimen, never on the leaf: `agent.message.plain` is the ordinary assistant
    // message, and exempting the LEAF would exempt the whole population this sweep exists
    // to guard. Each is named with its reason, because an unemittable leaf is a defect
    // until someone writes down why it is not one.
    const NO_TEXT_TO_EMIT: &[(&str, &str)] = &[
        (
            "image",
            "an image-only record is a picture: a pattern scan has nothing to match or \
             excerpt, and `csift image` is the surface that reads it",
        ),
        (
            "fallback",
            "the API served the turn from a fallback model and wrote no text; the leaf \
             records that the turn happened, and there is no body to emit",
        ),
    ];

    let mut swept = 0usize;
    for (value, line) in crate::model::specimens::BLOCK_SPECIMENS
        .iter()
        .chain(crate::model::specimens::TEMPLATE_SPECIMENS.iter())
    {
        let r = rec(line);
        let leaves = r.classify(&ClassifyCtx::top_level());
        let visible: Vec<Class> = leaves.iter().copied().filter(|c| c.llm_visible()).collect();
        if visible.is_empty() {
            continue;
        }
        if NO_TEXT_TO_EMIT.iter().any(|(v, _)| v == value) {
            continue;
        }
        swept += 1;
        let m = build_matcher(&args("")).unwrap();
        let mut hits = Vec::new();
        collect_record_hits(
            &r,
            None,
            None,
            LabelFilter::all(),
            &m,
            false,
            EXCERPT_MAX,
            &PlanIndex::default(),
            &HashMap::new(),
            &ClassifyCtx::top_level(),
            &mut hits,
        );
        assert_eq!(
            hits.len(),
            1,
            "{visible:?} is visible, so a flagless scan emits it exactly once - got {:?} \
             for {line}",
            hits.iter().map(|h| h.class).collect::<Vec<_>>()
        );
    }
    // EXACT, like every other drift guard here: 25 specimen rows (8 block, 17 template),
    // of which 2 take the `visible.is_empty()` arm above - the `document` block, which
    // carries no leaf at all, and `redacted_thinking`, whose leaf is invisible by
    // construction - and 2 are the exemptions named above, leaving 21. A `>=` bound would
    // pass while a row quietly stopped being swept, which is the drift this guard exists
    // to catch; ADDING a specimen means updating the number here and naming the new row.
    assert_eq!(
        swept, 21,
        "every specimen carrying a visible leaf is swept, minus the two named exemptions"
    );
}
