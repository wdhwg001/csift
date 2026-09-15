//! The columns a READER sees - the gate, the LLM answer, the human answer - and the
//! two accessors that render a row. A column that lags the code misinforms whoever
//! reached for that leaf, so each is held to the thing it describes.

use super::*;

#[test]
fn only_the_never_convert_payloads_disagree_with_their_leaf_default() {
    // A row's `llm` is the SHAPE's own conversion answer; a leaf's is the default for
    // records carrying it. They agree everywhere except where the harness drops a
    // payload whose leaf is otherwise visible - and that set is enumerated, not implied.
    let mut disagree: Vec<&str> = Vec::new();
    for r in rows() {
        if let Some(leaf) = r.leaf {
            if r.llm != leaf.llm_visible() {
                disagree.push(r.value());
            }
        }
    }
    disagree.sort_unstable();
    let mut want: Vec<&str> = NEVER_CONVERTS
        .iter()
        .copied()
        .filter(|v| {
            rows()
                .find(|r| r.value() == *v)
                .and_then(|r| r.leaf)
                .is_some_and(Class::llm_visible)
        })
        .collect();
    want.sort_unstable();
    assert_eq!(disagree, want, "the disagreeing rows are not the drop arms");
    assert_eq!(NEVER_CONVERTS.len(), 9, "the never-convert set drifted");
    // Every never-convert type is a real attachment row, and none of them converts.
    for v in NEVER_CONVERTS {
        let r = rows()
            .find(|r| matches!(r.shape, Shape::AttachmentType(a) if a == *v))
            .unwrap_or_else(|| panic!("{v} is not an attachment row"));
        assert!(!r.llm, "{v} claims a conversion it does not have");
    }
}

/// The rows whose leaf is decided on a DEEPER axis than their own, hand-kept with the
/// axis that decides it. Every OTHER leafless row is a shape csift models nothing for,
/// and an address is the only way to it.
const LEAF_DECIDED_DEEPER: &[(&str, &str)] = &[
    (
        "user",
        "the record's own content: a template, a block shape or its prose",
    ),
    ("assistant", "the content blocks it carries"),
    ("system", "its subtype"),
    ("attachment", "its payload type"),
    (
        "task-notification-frame",
        "the kind of the `<task-notification>` pulse the frame wraps",
    ),
];

/// The GATE column is what SPEC 5.3 and both SKILL tables print, so a row whose gate
/// lags the code misinforms a reader at exactly the leaf they reached for. The two
/// default-on VALUE needles are the one place the column and the scan can disagree
/// silently: nothing else derives from `Gate::DefaultNeedle`, so only this test holds it
/// to the needle table.
///
/// Both directions, because a one-way check is how the column lagged in the first place.
#[test]
fn the_default_needle_gate_names_exactly_the_default_on_needles() {
    let needles: Vec<String> = crate::cli::attachment_view_needles()
        .into_iter()
        .map(|(n, _)| n.trim_matches('"').to_string())
        .collect();
    let gated: Vec<String> = rows()
        .filter(|r| r.gate == Gate::DefaultNeedle)
        .map(|r| r.value().to_string())
        .collect();
    for value in &gated {
        assert!(
            needles.contains(value),
            "{value} is marked DefaultNeedle but no needle admits it on a flagless scan"
        );
    }
    for needle in &needles {
        let row = rows()
            .find(|r| r.value() == needle.as_str())
            .unwrap_or_else(|| panic!("the needle {needle} names no inventory row"));
        assert_eq!(
            row.gate,
            Gate::DefaultNeedle,
            "{} has a default-on needle, so its gate column must say so",
            row.disk()
        );
    }
    assert_eq!(
        gated.len(),
        needles.len(),
        "one row per needle: rows {gated:?} against needles {needles:?}"
    );
}

#[test]
fn a_row_with_no_leaf_is_reachable_only_by_an_address() {
    for r in rows() {
        if r.leaf.is_some() {
            continue;
        }
        // A row with no leaf can only be reached by an address: no selector can name it.
        // The exceptions are the rows whose leaf is decided DEEPER, so the selectors that
        // reach those deeper leaves admit the line - and they are named, not a category.
        if let Some((_, axis)) = LEAF_DECIDED_DEEPER.iter().find(|(v, _)| *v == r.value()) {
            assert_ne!(
                r.gate,
                Gate::Address,
                "{} is decided by {axis}, so a selector reaching a deeper leaf admits it \
                 and an address is not the only way in",
                r.disk()
            );
            continue;
        }
        assert_eq!(r.gate, Gate::Address, "{} has no leaf to select", r.disk());
    }
    // And the exemption list is exhaustive in the other direction: nothing on it carries
    // a leaf of its own, which would make the entry a stale excuse.
    for (value, _) in LEAF_DECIDED_DEEPER {
        let row = rows()
            .find(|r| r.value() == *value)
            .unwrap_or_else(|| panic!("{value} names no inventory row"));
        assert!(row.leaf.is_none(), "{value} has a leaf of its own now");
    }
}

/// `e` is a `const fn`, so every one of the 105 table rows is built by the COMPILER and
/// no test reaches the constructor at runtime. It builds a five-field struct out of five
/// positional arguments, which is exactly the shape a column swap hides in, so the
/// mapping from argument position to field is pinned against a real row: rebuild the
/// `queue-operation` row from its own recorded columns and require the two to answer
/// identically on every accessor.
#[test]
fn the_row_constructor_maps_each_argument_to_its_own_column() {
    let table = rows()
        .find(|r| matches!(r.shape, Shape::TopLevelType("queue-operation")))
        .expect("the queue-operation row");
    let built = e(table.shape, table.leaf, table.gate, table.llm, table.human);
    assert_eq!(built.value(), table.value());
    assert_eq!(built.disk(), table.disk());
    assert_eq!(built.leaf, table.leaf);
    assert_eq!(built.gate, table.gate);
    assert_eq!(built.llm, table.llm);
    assert_eq!(built.human, table.human);
    // ... and the first argument really is the SHAPE: the same value on another axis
    // renders another disk string, so a swap could not pass the equality above.
    let other_axis = e(
        Shape::SystemSubtype("queue-operation"),
        table.leaf,
        table.gate,
        table.llm,
        table.human,
    );
    assert_ne!(other_axis.disk(), table.disk());
}

#[test]
fn the_disk_string_is_derived_from_the_shape() {
    let top = rows()
        .find(|r| r.value() == "cost-state")
        .expect("the cost-state row");
    assert_eq!(top.disk(), r#"type:"cost-state""#);
    let sub = rows()
        .find(|r| matches!(r.shape, Shape::SystemSubtype("api_error")))
        .expect("the api_error row");
    assert_eq!(sub.disk(), r#"type:"system" subtype:"api_error""#);
    let att = rows()
        .find(|r| matches!(r.shape, Shape::AttachmentType("edited_text_file")))
        .expect("the edited_text_file row");
    assert_eq!(
        att.disk(),
        r#"type:"attachment" attachment.type:"edited_text_file""#
    );
    let blk = rows()
        .find(|r| matches!(r.shape, Shape::Block("tool_use")))
        .expect("the tool_use row");
    assert_eq!(blk.disk(), r#"message.content[] type:"tool_use""#);
    let tpl = rows()
        .find(|r| matches!(r.shape, Shape::IsMetaFamily("resume-prompt")))
        .expect("the resume-prompt row");
    assert_eq!(
        tpl.disk(),
        r#"type:"user" isMeta:true template:resume-prompt"#
    );

    // THREE shapes whose on-disk mark is not the axis value alone, because a second fact
    // decides which leaf the line carries. These strings are what SPEC 5.3 and both SKILL
    // tables print, so each names the field a reader greps for.
    let queued = rows()
        .find(|r| matches!(r.shape, Shape::AttachmentType("queued_command")))
        .expect("the queued_command row");
    assert!(
        queued.disk().contains("attachment.origin.kind") && queued.disk().contains("user.message"),
        "the queued row names the field that makes it the person's own: {}",
        queued.disk()
    );
    let fire = rows()
        .find(|r| matches!(r.shape, Shape::IsMetaFamily("schedule-fire")))
        .expect("the schedule-fire row");
    assert!(
        fire.disk().contains("parentUuid") && !fire.disk().contains("template:"),
        "the fired prompt has no template - its mark is the parent link: {}",
        fire.disk()
    );
    let img = rows()
        .find(|r| matches!(r.shape, Shape::Block("image")))
        .expect("the image row");
    assert!(
        img.disk().contains("image-ONLY") && img.disk().contains("[Image: "),
        "the image row names the SHAPE and its text form: {}",
        img.disk()
    );
}
