//! Inventory drift guards: the per-axis counts, the shape uniqueness, the partition of
//! the leaf space, and the agreement between this table and the classify family tables
//! it records. Every expectation is typed out by hand; a test that derived its numbers
//! from the table would pass on any drift.

use super::*;

/// The hand-kept per-axis row counts: 22 top-level types, 11 system subtypes, 49
/// attachment payload types, 8 content block types, 15 isMeta template families.
const COUNTS: [(&str, usize); 5] = [
    ("top-level type", 22),
    ("system subtype", 11),
    ("attachment type", 49),
    ("block type", 8),
    ("isMeta template", 15),
];

fn kind_of(s: Shape) -> &'static str {
    match s {
        Shape::TopLevelType(_) => "top-level type",
        Shape::SystemSubtype(_) => "system subtype",
        Shape::AttachmentType(_) => "attachment type",
        Shape::Block(_) => "block type",
        Shape::IsMetaFamily(_) => "isMeta template",
    }
}

#[test]
fn each_axis_holds_the_pinned_number_of_rows() {
    for (kind, want) in COUNTS {
        let got = rows().filter(|r| kind_of(r.shape) == kind).count();
        assert_eq!(got, want, "{kind} row count drifted");
    }
    assert_eq!(rows().count(), COUNTS.iter().map(|c| c.1).sum::<usize>());
    // The five tables are in axis order, so a flat walk visits each axis in one run.
    let mut seen: Vec<&str> = Vec::new();
    for r in rows() {
        if seen.last() != Some(&kind_of(r.shape)) {
            seen.push(kind_of(r.shape));
        }
    }
    assert_eq!(
        seen,
        COUNTS.iter().map(|c| c.0).collect::<Vec<_>>(),
        "the axis order drifted"
    );
}

#[test]
fn no_shape_value_repeats_on_its_own_axis() {
    for (kind, _) in COUNTS {
        let mut vals: Vec<&str> = rows()
            .filter(|r| kind_of(r.shape) == kind)
            .map(|r| r.value())
            .collect();
        vals.sort_unstable();
        let n = vals.len();
        vals.dedup();
        assert_eq!(vals.len(), n, "a {kind} value is listed twice");
    }
}

#[test]
fn the_shaped_and_unshaped_leaves_partition_the_taxonomy() {
    let shaped: Vec<Class> = rows().filter_map(|r| r.leaf).collect();
    for &c in &shaped {
        assert!(
            Class::ALL.contains(&c),
            "{c:?} is an inventory leaf outside Class::ALL"
        );
    }
    for (c, reason) in UNSHAPED {
        assert!(
            !shaped.contains(c),
            "{c:?} is listed as unshaped and also carried by a shape row"
        );
        assert!(!reason.is_empty(), "{c:?} carries no reason");
    }
    for &c in Class::ALL {
        let covered = shaped.contains(&c) || UNSHAPED.iter().any(|(u, _)| *u == c);
        assert!(
            covered,
            "{} has neither a shape row nor an UNSHAPED reason",
            c.path()
        );
    }
    assert_eq!(UNSHAPED.len(), 22, "the unshaped table drifted");
}

#[test]
fn every_row_agrees_with_the_classify_family_table_it_records() {
    // The inventory is a RECORD of the mapping, never a second one: each row is walked
    // back through the family table that actually decides the leaf.
    for r in rows() {
        match r.shape {
            Shape::TopLevelType(t) => {
                // The four transcript types carry no leaf of their own; the other 18 do.
                let want = crate::model::classify::bookkeeping::classify(t);
                assert_eq!(r.leaf, want, "top-level type {t}");
            }
            Shape::SystemSubtype(s) => {
                assert_eq!(r.leaf, crate::model::classify::system::classify(s), "{s}");
            }
            Shape::AttachmentType(a) => {
                assert_eq!(
                    r.leaf,
                    crate::model::classify::attachments::classify(a),
                    "{a}"
                );
            }
            // The block and template axes are decided with more than a value in hand (a
            // signature, the record's own text), so they are pinned by the family tests.
            Shape::Block(_) | Shape::IsMetaFamily(_) => {}
        }
    }
}

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

#[test]
fn a_row_with_no_leaf_is_reachable_only_by_an_address() {
    for r in rows() {
        // A row with no leaf can only be reached by an address: no selector can name it.
        // The four TRANSCRIPT types are the exception - their leaf is decided deeper, so
        // the selectors that reach those deeper leaves admit the line.
        if r.leaf.is_none() && !matches!(r.shape, Shape::TopLevelType(_)) {
            assert_eq!(r.gate, Gate::Address, "{} has no leaf to select", r.disk());
        }
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
}
