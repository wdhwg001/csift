//! The TABLE itself: how many rows sit on each axis, that no value repeats on its
//! own axis, that the shaped and unshaped leaves partition `Class::ALL`, and that
//! every row agrees with the family table that decides its leaf.

use super::*;

/// The hand-kept per-axis row counts: 22 top-level types, 11 system subtypes, 50
/// attachment payload types, 8 content block types, 17 isMeta template families.
const COUNTS: [(&str, usize); 5] = [
    ("top-level type", 22),
    ("system subtype", 11),
    ("attachment type", 50),
    ("block type", 8),
    ("isMeta template", 17),
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
    assert_eq!(UNSHAPED.len(), 21, "the unshaped table drifted");
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
            // The block and template axes need a whole RECORD, not a value, so their rows
            // are walked back through the engine itself by the two tests below.
            Shape::Block(_) | Shape::IsMetaFamily(_) => {}
        }
    }
}

// -- The RENDERED table: SPEC section 5 and this inventory are one answer --

/// How the rendered SPEC table spells each axis.
fn spec_kind(s: Shape) -> &'static str {
    match s {
        Shape::TopLevelType(_) => "TopLevelType",
        Shape::SystemSubtype(_) => "SystemSubtype",
        Shape::AttachmentType(_) => "AttachmentType",
        Shape::Block(_) => "Block",
        Shape::IsMetaFamily(_) => "IsMetaFamily",
    }
}

/// How the rendered SPEC table spells each gate.
fn spec_gate(g: Gate) -> &'static str {
    match g {
        Gate::Default => "default scan",
        Gate::Selector => "explicit -t",
        Gate::Attachments => "-t or --attachments",
        Gate::DefaultNeedle => "default scan (own needle)",
        Gate::Address => "show address only",
    }
}

/// How the rendered SPEC table spells each human verdict. `Never` renders `no` rather
/// than the enum's word, because the column asks "does a person see this".
fn spec_human(h: Human) -> &'static str {
    match h {
        Human::Always => "yes",
        Human::Verbose => "verbose",
        Human::Indirect => "indirect",
        Human::Never => "no",
        Human::Unstated => "(unstated)",
    }
}

/// One inventory row as the SPEC table prints it.
fn spec_row(r: &Entry) -> String {
    let leaf = match r.leaf {
        Some(c) => format!("`{}`", c.path()),
        None => "(deeper axis / unmodeled)".to_string(),
    };
    format!(
        "| {} | `{}` | {leaf} | {} | {} | {} |",
        spec_kind(r.shape),
        r.value(),
        spec_gate(r.gate),
        if r.llm { "yes" } else { "no" },
        spec_human(r.human)
    )
}

/// SPEC section 5's inventory table IS this table, row for row and column for column.
///
/// The table is documentation of a mapping, not a second mapping, so a hand-edited cell
/// is a lie about csift rather than a stale doc - the sharpest case being the HUMAN
/// column, where a wrong value tells a reader that a record they never saw was on their
/// screen. Comparing the rendered form (rather than a count, or the leaf alone) is what
/// makes every column load-bearing: gate, LLM and human each fail here on their own.
#[test]
fn the_spec_inventory_table_is_this_table() {
    let spec = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SPEC.md"))
        .expect("SPEC.md at the repo root");
    let printed: Vec<&str> = spec
        .lines()
        .map(str::trim_end)
        .filter(|l| {
            [
                "TopLevelType",
                "SystemSubtype",
                "AttachmentType",
                "Block",
                "IsMetaFamily",
            ]
            .iter()
            .any(|k| l.starts_with(&format!("| {k} |")))
        })
        .collect();
    let want: Vec<String> = rows().map(spec_row).collect();
    assert_eq!(
        printed.len(),
        want.len(),
        "SPEC renders {} inventory rows, the table holds {}",
        printed.len(),
        want.len()
    );
    for (got, want) in printed.iter().zip(&want) {
        assert_eq!(got, want, "SPEC row disagrees with the inventory");
    }
}
