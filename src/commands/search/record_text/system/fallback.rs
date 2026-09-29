//! The turn the API served from a FALLBACK model.
//!
//! The API can answer an assistant turn with a different model and says so with a
//! `{type:"fallback", from:{model}, to:{model}}` content block written IN PLACE of the
//! text. So the record reaches the transcript carrying that block and nothing else, and
//! `from.model` is the only place the pre-fallback model survives: every other record of
//! the same API message already names the model that answered.
//!
//! csift puts both ids in the LABEL ZONE of whatever unit the record emits, as
//! `[served by <to.model>, fell back from <from.model>]` plus the JSON keys `served_by`
//! and `fell_back_from`. The ids come off the PARSED BLOCK rather than off the source
//! line, and that is the whole reason `Block::Fallback` models `from` and `to`: a scan
//! hands a family no source line for a role-bearing record, so a marker read off the line
//! would appear under `show --line` and vanish under `search` - an address-versus-scan
//! split in the facts the one stamping site exists to prevent. A block naming only one
//! side yields no marker at all: half of it would name the wrong model.
//!
//! On disk the block is the SOLE block of its record, so the record has no text of its own.
//! It emits ONE `agent.message.plain` unit anyway, on a scan as under an address, and its
//! body is the marker's own words ([`fallback_body`]): without a unit the marker had
//! nothing to ride, so `served_by` answered under `show --line` and on no scan at all, and
//! `-t agent.message.plain` could not find a turn it labels. A record that fell back AND
//! spoke carries the marker on its text unit and emits no second one.
//!
//! It lives in the SYSTEM family's directory because the same event wrote both records:
//! the block, and the `model_refusal_fallback` banner below it that this family already
//! renders. The producer differs from the event records around it (this block comes from
//! the API inside an assistant message), which is why it is its own file.

use super::*;

/// The two model ids of an assistant record's `fallback` block: `from.model` (the model
/// the turn was asked of) and `to.model` (the model that served it). `None` when the
/// record carries no such block or either id is missing - the marker exists to name both,
/// and half of it would name the wrong one.
pub(crate) fn fallback_block_models(rec: &Record) -> Option<(String, String)> {
    for b in rec.blocks()? {
        let Block::Fallback { from, to } = b else {
            continue;
        };
        let side = |s: &Option<crate::model::FallbackSide>| {
            s.as_ref()
                .and_then(|s| s.model.clone())
                .filter(|m| !m.is_empty())
        };
        if let (Some(from), Some(to)) = (side(from), side(to)) {
            return Some((from, to));
        }
    }
    None
}

/// The BODY of the unit a fallback-only record emits: the marker's own words, so a scan
/// for `fell back` or a model id reaches it. `None` when the record also carries a text
/// block (that unit carries the marker, and a second unit would double-count the turn) or
/// no readable block. The words are fabricated - only the ids are bytes of the line - so the
/// stage-2 gate re-renders this same function behind the `"fallback"` synth marker.
pub(crate) fn fallback_body(rec: &Record) -> Option<String> {
    if rec
        .blocks()?
        .iter()
        .any(|b| matches!(b, Block::Text { .. }))
    {
        return None;
    }
    let (from, to) = fallback_block_models(rec)?;
    Some(format!("served by {to}, fell back from {from}"))
}

/// The label-zone marker, the two exposed keys and the banner EDGE of a fallback-served
/// turn. Empty for every record carrying no readable block, so a caller never renders an
/// invented model id.
pub(crate) fn notes(rec: &Record) -> (Vec<HitFact>, Vec<Marker>) {
    let Some((from, to)) = fallback_block_models(rec) else {
        return (Vec::new(), Vec::new());
    };
    (
        vec![
            HitFact::System(Fact::Fallback {
                served_by: to.clone(),
                fell_back_from: from.clone(),
            }),
            // The banner is the `model_refusal_fallback` record that DESCENDS from this
            // one, and descent is the one direction no uuid map answers: this record names
            // no key pointing down, and one record can have several banners, so the answer
            // is a SET rather than the single target an edge carries. The index csift
            // holds does not close it either - it maps a `tool_use_id` to a line, which is
            // a different join. So the edge is emitted with its reason rather than omitted
            // - a reader learns that the banner exists, that it names this record, and
            // which way to look.
            HitFact::ForUnknown {
                kind: EdgeKind::Banner,
                reason: "the banner names this record, not the reverse; a one-to-many \
                         descent, which no uuid map answers"
                    .to_string(),
            },
        ],
        vec![Marker {
            head: "served by",
            detail: Some(format!("{to}, fell back from {from}")),
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(line: &str) -> Record {
        serde_json::from_str(line).expect("fixture parses")
    }

    fn seen(rec: &Record) -> (Vec<String>, Vec<(&'static str, String)>) {
        let ctx = ClassifyCtx::top_level();
        let (facts, markers) = facts(rec, None, &Section::record(None), &ctx);
        (
            markers.iter().map(Marker::render).collect(),
            facts
                .iter()
                .flat_map(|f| {
                    f.json()
                        .into_iter()
                        .map(|(k, v)| (k, v.to_string()))
                        .collect::<Vec<_>>()
                })
                .collect(),
        )
    }

    /// The fallback-served turn: both model ids in the marker, the two exposed keys and
    /// the honest banner edge - all read off the PARSED BLOCK, so they ride a unit with
    /// no source line, which is what a scan hands a family for an assistant record.
    #[test]
    fn a_fallback_served_turn_names_both_models_without_a_source_line() {
        let line = r#"{"type":"assistant","uuid":"a1","message":{"role":"assistant","model":"model-b","content":[{"type":"fallback","from":{"model":"model-a"},"to":{"model":"model-b"}}]}}"#;
        let r = rec(line);
        assert_eq!(
            fallback_block_models(&r),
            Some(("model-a".to_string(), "model-b".to_string()))
        );
        let (markers, keys) = seen(&r);
        assert_eq!(markers, vec!["[served by model-b, fell back from model-a]"]);
        assert_eq!(
            keys,
            vec![
                ("served_by", "\"model-b\"".to_string()),
                ("fell_back_from", "\"model-a\"".to_string()),
                (
                    "for",
                    r#"[{"basis":null,"kind":"banner","reason":"the banner names this record, not the reverse; a one-to-many descent, which no uuid map answers","target":null}]"#
                        .to_string()
                ),
            ]
        );
    }

    /// A turn that fell back AND spoke keeps its text and STILL carries the marker: the
    /// fact is about the whole turn, so every unit the record emits names both models.
    #[test]
    fn a_turn_that_fell_back_and_spoke_carries_the_marker_too() {
        let spoke = r#"{"type":"assistant","uuid":"a3","message":{"role":"assistant","content":[{"type":"fallback","from":{"model":"model-a"},"to":{"model":"model-b"}},{"type":"text","text":"here"}]}}"#;
        let (markers, _) = seen(&rec(spoke));
        assert_eq!(markers, vec!["[served by model-b, fell back from model-a]"]);
    }

    /// A block naming one side, an empty id, a record with no blocks and a record with no
    /// fallback block all carry nothing: half a marker would name the wrong model.
    #[test]
    fn a_half_named_block_yields_no_marker() {
        for line in [
            r#"{"type":"assistant","uuid":"a2","message":{"role":"assistant","content":[{"type":"fallback","to":{"model":"model-b"}}]}}"#,
            r#"{"type":"assistant","uuid":"a5","message":{"role":"assistant","content":[{"type":"fallback","from":{"model":""},"to":{"model":"model-b"}}]}}"#,
            r#"{"type":"assistant","uuid":"a6","message":{"role":"assistant","content":[{"type":"fallback","from":{},"to":{}}]}}"#,
            r#"{"type":"assistant","uuid":"a4"}"#,
            r#"{"type":"assistant","uuid":"a7","message":{"role":"assistant","content":[{"type":"text","text":"plain"}]}}"#,
        ] {
            let r = rec(line);
            assert_eq!(fallback_block_models(&r), None, "{line}");
            let (markers, keys) = seen(&r);
            assert!(markers.is_empty() && keys.is_empty(), "{line}");
        }
    }
}
