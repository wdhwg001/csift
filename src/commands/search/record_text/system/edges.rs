//! The PROVENANCE EDGES a system event names: which other record the event is FOR.
//!
//! Three of the family's leaves carry one, and each names its target a DIFFERENT way - a
//! uuid the harness wrote (`refusedUserMessageUuid`), the chain link (`parentUuid`), a
//! foreign join key (`taskId`). The edge is emitted whether or not the key is there:
//! `[for: <kind> unknown (<reason>)]` says csift looked and the line did not say, where a
//! missing tag would say only that csift was silent.
//!
//! The two UUID edges are built through [`super::super::for_edge`], the one place the
//! uuid-to-line lookup happens, so every family's edge resolves and renders alike. The
//! third is not that shape and says so: the scheduler's `taskId` is not a transcript uuid
//! and the record it names is found by MATCHING an id inside a tool result, which a uuid
//! map cannot answer - so the id rides the hit as a key and the head, and the edge
//! carries the reason a reader needs to make the join by hand.

use super::*;

/// The edge this leaf carries, or nothing when the leaf carries none.
pub(crate) fn edge(
    class: Class,
    rec: &Record,
    p: Option<&serde_json::Map<String, serde_json::Value>>,
    ctx: &ClassifyCtx,
) -> Option<HitFact> {
    match class {
        // The rewind target the harness itself names: a uuid that resolves to a record in
        // this same transcript, so the edge fetches the message the refused request was
        // for. Its sibling `retractedMessageUuids` is deliberately NOT an edge - those
        // are WIRE uuids of the normalized messages the fallback retracted and they
        // resolve to no line, so offering them beside a resolvable one would invite a
        // join that cannot succeed.
        Class::SystemRefusal => {
            let key = p.and_then(|m| str_field(m, "refusedUserMessageUuid"));
            Some(super::super::for_edge(
                "refused",
                Basis::Key,
                key.as_deref(),
                None,
                ctx,
            ))
        }
        // The slash command this stdout record is the OUTPUT of. The harness writes the
        // caveat, then the wrapper, then this record parented to the wrapper - but a RUN
        // of local commands chains stdout to stdout, so what the link names is the record
        // ABOVE rather than the wrapper by construction. Hence `Basis::Position`: this is
        // csift reading the file's shape, not a key the harness wrote for the purpose.
        Class::SystemCommand => {
            let parent = rec.parent_uuid.as_deref().filter(|u| !u.is_empty());
            Some(super::super::for_edge(
                "wraps",
                Basis::Position,
                parent,
                None,
                ctx,
            ))
        }
        // The call that armed this schedule. `taskId` is the scheduler's own id, not a
        // transcript uuid, and the record that answers is a tool RESULT whose own `id`
        // equals it - a match rather than an address, which the chain's uuid map cannot
        // make. So the relation is stated with its reason and the id itself rides the
        // head and the hit's `task_id` key, where `csift search '<task id>'` reaches
        // both this record and the arming call. The `/loop` wakeup arm arms no task and
        // carries no such key at all.
        Class::ScheduleEvent => Some(HitFact::ForUnknown {
            kind: "armed_by",
            reason: match p.and_then(|m| str_field(m, "taskId")) {
                Some(_) => "the arming call is joined by taskId, not by a record uuid - \
                            search the id"
                    .to_string(),
                None => "the record carries no taskId - the wakeup arm arms no task".to_string(),
            },
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn rec(line: &str) -> Record {
        serde_json::from_str(line).expect("fixture parses")
    }

    fn payload(line: &str) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::from_str(line) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => panic!("fixture parses as an object"),
        }
    }

    /// A lookup over a hand-built uuid -> line map, so the resolved form is exercised
    /// without a scan.
    struct Lines(HashMap<String, usize>);
    impl crate::model::LineOfUuid for Lines {
        fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
            self.0.get(uuid).copied()
        }
    }

    fn zone(f: &HitFact) -> String {
        f.label_zone().expect("an edge always renders a tag")
    }

    fn json_of(f: &HitFact) -> serde_json::Value {
        let pairs = f.json();
        assert_eq!(pairs.len(), 1, "an edge projects one key");
        assert_eq!(pairs[0].0, "for");
        pairs[0].1.clone()
    }

    /// The refusal edge: a uuid this file holds renders its LINE, a uuid it does not hold
    /// renders the id with the reason, and a null or absent key says the record named
    /// nothing rather than dropping the relation.
    #[test]
    fn the_refusal_edge_resolves_or_says_why_not() {
        let here = r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r1","refusedUserMessageUuid":"00000000-1111-4222-8333-444444444444"}"#;
        let lines = Lines(HashMap::from([(
            "00000000-1111-4222-8333-444444444444".to_string(),
            12usize,
        )]));
        let mut ctx = ClassifyCtx::top_level();
        ctx.lines = Some(&lines);
        let f =
            edge(Class::SystemRefusal, &rec(here), Some(&payload(here)), &ctx).expect("an edge");
        assert_eq!(zone(&f), "[for: refused L12]");
        assert_eq!(json_of(&f)[0]["target"]["line"], serde_json::json!(12));
        assert_eq!(json_of(&f)[0]["basis"], serde_json::json!("key"));

        // The SAME record with no row space: the id stands in for the line, cut to the
        // eight-character handle, and the edge says why there is no line.
        let bare = ClassifyCtx::top_level();
        let f = edge(
            Class::SystemRefusal,
            &rec(here),
            Some(&payload(here)),
            &bare,
        )
        .expect("an edge");
        assert_eq!(zone(&f), "[for: refused 00000000 (not in this file)]");

        let null = r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r2","refusedUserMessageUuid":null}"#;
        let f = edge(
            Class::SystemRefusal,
            &rec(null),
            Some(&payload(null)),
            &bare,
        )
        .expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: refused unknown (the record carries no such key)]"
        );
        assert_eq!(json_of(&f)[0]["target"], serde_json::Value::Null);

        let missing = r#"{"type":"system","subtype":"model_refusal_no_fallback","uuid":"r3"}"#;
        let f = edge(
            Class::SystemRefusal,
            &rec(missing),
            Some(&payload(missing)),
            &bare,
        )
        .expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: refused unknown (the record carries no such key)]"
        );
        // No source line at all takes the same honest-absent road.
        let f = edge(Class::SystemRefusal, &rec(here), None, &bare).expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: refused unknown (the record carries no such key)]"
        );
        // An EMPTY string is a key that names nothing, not a key that names "": it takes
        // the absent road rather than rendering a blank target a reader would try to
        // resolve. (Without this the emptiness guard has no case behind it.)
        let empty = r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r4","refusedUserMessageUuid":""}"#;
        let f = edge(
            Class::SystemRefusal,
            &rec(empty),
            Some(&payload(empty)),
            &bare,
        )
        .expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: refused unknown (the record carries no such key)]"
        );
    }

    /// The delivered command's edge is the CHAIN link, read off the record rather than
    /// off a payload key - so it resolves with no source line at all.
    #[test]
    fn the_command_edge_reads_the_chain_link() {
        let lines = Lines(HashMap::from([("w1".to_string(), 7usize)]));
        let mut ctx = ClassifyCtx::top_level();
        ctx.lines = Some(&lines);
        let cmd = r#"{"type":"system","subtype":"local_command","uuid":"c1","parentUuid":"w1","content":"ok"}"#;
        let f = edge(Class::SystemCommand, &rec(cmd), None, &ctx).expect("an edge");
        assert_eq!(zone(&f), "[for: wraps L7]");
        assert_eq!(json_of(&f)[0]["basis"], serde_json::json!("position"));
        let orphan = r#"{"type":"system","subtype":"local_command","uuid":"c2","content":"ok"}"#;
        let f = edge(Class::SystemCommand, &rec(orphan), None, &ctx).expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: wraps unknown (the record carries no such key)]"
        );
        // An EMPTY `parentUuid` names nothing either, and reads the same way a missing
        // one does rather than rendering a blank target.
        let blank = r#"{"type":"system","subtype":"local_command","uuid":"c3","parentUuid":"","content":"ok"}"#;
        let f = edge(Class::SystemCommand, &rec(blank), None, &ctx).expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: wraps unknown (the record carries no such key)]"
        );
    }

    /// The fire record's edge is the one that is NOT a uuid: it states the relation and
    /// the join, and the two arms differ in whether the key is there at all.
    #[test]
    fn the_schedule_edge_states_a_join_a_uuid_map_cannot_make() {
        let ctx = ClassifyCtx::top_level();
        let cron = r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s1","content":"Running scheduled task (Jun 7 5:28am)","taskId":"task-zz"}"#;
        let f =
            edge(Class::ScheduleEvent, &rec(cron), Some(&payload(cron)), &ctx).expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: armed_by unknown (the arming call is joined by taskId, not by a record \
             uuid - search the id)]"
        );
        assert_eq!(json_of(&f)[0]["target"], serde_json::Value::Null);
        let wakeup = r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s2","content":"Claude resuming /loop wakeup (Jun 7 6:40am)"}"#;
        let f = edge(
            Class::ScheduleEvent,
            &rec(wakeup),
            Some(&payload(wakeup)),
            &ctx,
        )
        .expect("an edge");
        assert_eq!(
            zone(&f),
            "[for: armed_by unknown (the record carries no taskId - the wakeup arm arms no task)]"
        );
    }

    /// The two leaves with no edge carry none - an absent edge is a leaf's property,
    /// never a record's missing value.
    #[test]
    fn the_edgeless_leaves_carry_no_edge() {
        let line = r#"{"type":"system","subtype":"informational","uuid":"n1","level":"warning","content":"x"}"#;
        let ctx = ClassifyCtx::top_level();
        for class in [
            Class::SystemNotice,
            Class::SystemKill,
            Class::SystemApiError,
        ] {
            assert!(
                edge(class, &rec(line), Some(&payload(line)), &ctx).is_none(),
                "{class:?}"
            );
        }
    }
}
