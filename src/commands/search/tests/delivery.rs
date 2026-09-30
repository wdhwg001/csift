//! The `[not delivered]` tag against the JSON `delivered` field: one verdict, two
//! projections, swept over every shape the inventory records.

use super::*;

/// The units a record yields under a wide-open filter and an empty pattern - what a scan
/// that selects every leaf emits for it - with `opener` standing in for the scan layer's
/// abandoned-opener verdict (`user.unsent` / `user.rewound`), which no record carries
/// on its own.
fn units(line: &str, opener: Option<Class>) -> Vec<Hit> {
    let r = rec(line);
    let m = build_matcher(&args("")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        Some(line),
        opener,
        LabelFilter::all(),
        &m,
        false,
        EXCERPT_MAX,
        &CallIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    hits
}

/// Every unit whose JSON `delivered` reads false renders `[not delivered]` in its label
/// zone, and no delivered unit does - over one line per inventory row of the three value
/// axes, the block and template specimens, a queued person's message and the two
/// abandoned-opener leaves. The text tag once printed only for a record carrying a
/// per-record delivery override, so a leaf the model never receives BY ITS LEAF (the turn
/// closers, a compaction boundary, a scheduler event, a kill, a queued or recalled
/// message) read false in JSON and untagged in text.
#[test]
fn the_not_delivered_tag_prints_exactly_where_delivered_reads_false() {
    let mut lines: Vec<(String, Option<Class>)> = crate::model::rows()
        .filter_map(|r| crate::model::specimens::value_line(r.shape))
        .map(|l| (l, None))
        .collect();
    lines.extend(
        crate::model::specimens::BLOCK_SPECIMENS
            .iter()
            .chain(crate::model::specimens::TEMPLATE_SPECIMENS.iter())
            .map(|(_, l)| (l.to_string(), None)),
    );
    // The person's own queued text: a `queue-operation` line WITH content is `user.queued`,
    // a shape the value axis above (a bare type) cannot reach.
    lines.push((
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:00:00.000Z","content":"zzqueued please continue"}"#
            .to_string(),
        None,
    ));
    let opener =
        r#"{"type":"user","uuid":"o1","message":{"role":"user","content":"zzdraft first try"}}"#;
    lines.push((opener.to_string(), Some(Class::UserUnsent)));
    lines.push((opener.to_string(), Some(Class::UserRewound)));

    let (mut undelivered, mut delivered) = (0usize, 0usize);
    let mut seen: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    for (line, over) in &lines {
        for h in units(line, *over) {
            let label = render_label(&h);
            let tagged = label.contains("[not delivered]");
            assert_eq!(
                tagged,
                !h.delivered(),
                "`{label}` - JSON delivered={} for {line}",
                h.delivered()
            );
            assert!(
                label.matches("[not delivered]").count() <= 1,
                "one tag, never two: `{label}`"
            );
            if h.delivered() {
                delivered += 1;
            } else {
                undelivered += 1;
                if let Some(c) = h.class {
                    seen.insert(c.path());
                }
            }
        }
    }
    // The leaves this sweep must reach undelivered - the ones the tag skipped - named, so a
    // specimen that stops producing one fails here rather than passing on the rest.
    for leaf in [
        "harness.turn.duration",
        "harness.turn.stop-hooks",
        "harness.turn.recap",
        "harness.compaction.boundary",
        "harness.schedule.event",
        "harness.system.kill",
        "user.queued",
        "user.unsent",
        "user.rewound",
        "harness.bookkeeping.last-prompt",
    ] {
        assert!(
            seen.contains(leaf),
            "{leaf} was not swept undelivered: {seen:?}"
        );
    }
    assert!(
        delivered > 0 && undelivered > 0,
        "{delivered} / {undelivered}"
    );
}
