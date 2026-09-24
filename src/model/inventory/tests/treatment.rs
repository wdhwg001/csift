//! What csift DOES with each inventory row, walked one synthetic line at a time.
//!
//! The table tests beside this one ask whether each row agrees with the family table that
//! decides its leaf, which is a VALUE-level question - a payload type in, a leaf out. This
//! file asks the record-level one: build a line from the row's own `disk` shape, hand it to
//! the real engine, and require the treatment the row advertises.
//!
//! Three statements, and each is a different way a row can be a lie. A row that names a
//! LEAF must produce it from a whole record, not only from its value. A row that names
//! NONE must produce the unmodeled unit, which is the only thing that keeps the 1:1 law
//! from being a bail. And a row in `NEVER_CONVERTS` must read `delivered: false` at the
//! RECORD level, because that is the verdict a bare selector consults - the `llm` COLUMN
//! is documentation and the override is what runs.

use super::*;

/// The leaf set a hand-built line classifies to, through the same entry point every
/// surface takes.
fn labels(line: &str) -> Vec<Class> {
    serde_json::from_str::<Record>(line)
        .expect("valid record")
        .classify(&ClassifyCtx::top_level())
}

/// One synthetic line per row of the three VALUE axes - a line type, a system subtype, an
/// attachment payload type - built from the row's own shape and nothing else.
fn line_for(shape: Shape) -> Option<String> {
    Some(match shape {
        // A non-transcript line: its own type is the whole shape. The extra key is there
        // so the line is a JSON object with content, which every real one is.
        Shape::TopLevelType(t) if !matches!(t, "user" | "assistant" | "system" | "attachment") => {
            format!(r#"{{"type":"{t}","zzprobe":"x"}}"#)
        }
        Shape::SystemSubtype(s) => {
            format!(r#"{{"type":"system","uuid":"zz1","subtype":"{s}","content":"zzprobe"}}"#)
        }
        Shape::AttachmentType(a) => format!(
            r#"{{"type":"attachment","uuid":"zz1","attachment":{{"type":"{a}","zzprobe":"x"}}}}"#
        ),
        // The four transcript types carry no leaf of their own and the block/template axes
        // need a record shape their own specimen tables already hold.
        _ => return None,
    })
}

/// Every row of the three value axes that names a LEAF produces it from a whole record.
///
/// `every_row_agrees_with_the_classify_family_table_it_records` asks the family table the
/// same question with a VALUE; this asks the engine with a RECORD, which is what a scan
/// actually does. The two differ wherever the engine looks at more than the value - the
/// two attachment views are exactly that case, and they are named here rather than
/// silently passing, because a row whose leaf needs a second field is a row a reader
/// cannot act on from the table alone.
#[test]
fn every_shaped_row_produces_its_leaf_from_a_whole_record() {
    // The payload types whose leaf depends on a SECOND fact the probe line does not
    // carry, each with the fact. Both are stated in the row's own `disk` string.
    const NEEDS_A_SECOND_FACT: &[(&str, &str)] = &[(
        "queued_command",
        "the payload's own origin.kind decides between user.message and harness.context.task",
    )];
    let mut checked = 0usize;
    for r in rows() {
        let Some(leaf) = r.leaf else { continue };
        let Some(line) = line_for(r.shape) else {
            continue;
        };
        if NEEDS_A_SECOND_FACT.iter().any(|(v, _)| *v == r.value()) {
            continue;
        }
        checked += 1;
        let got = labels(&line);
        assert!(
            got.contains(&leaf),
            "{} advertises {} - a record of that shape got {got:?}",
            r.disk(),
            leaf.path()
        );
    }
    // EXACT, like every other guard here: 18 non-transcript line types, 11 system
    // subtypes and 52 attachment payload types carry a leaf, less the one that needs a
    // second fact. A `>=` bound would pass while a row quietly stopped being walked.
    assert_eq!(checked, 80, "the walked set drifted");
}

/// A value NO row holds renders the UNMODELED unit, naming the value.
///
/// This is the 1:1 law seen from the table, and it is a statement about what is NOT in it:
/// every row of the three value axes carries a leaf, so the shapes that reach the unmodeled
/// unit are the ones a later Claude Code build will write and this table does not know yet.
/// `show --line N` never hard-misses a parseable line, and what it renders in place of a
/// label is the string `unmodeled_shape` builds - which has to NAME the value, because
/// `? unmodeled: system/some_new_subtype` tells a reader which shape csift has not caught
/// up with and a bare `? unmodeled` tells them nothing their own line did not.
///
/// The probe values are built by appending to a value the table DOES hold, so they cannot
/// collide with a real one and cannot stop being absent by accident.
#[test]
fn a_value_no_row_holds_renders_the_unmodeled_unit_naming_it() {
    let absent = |axis: fn(&Entry) -> bool, value: &str| {
        assert!(
            !rows().filter(|r| axis(r)).any(|r| r.value() == value),
            "{value} is in the table after all - the probe must name a value it does not hold"
        );
    };
    absent(
        |r| matches!(r.shape, Shape::SystemSubtype(_)),
        "zz_future_subtype",
    );
    absent(
        |r| matches!(r.shape, Shape::AttachmentType(_)),
        "zz_future_payload",
    );
    absent(
        |r| matches!(r.shape, Shape::TopLevelType(_)),
        "zz-future-line",
    );

    for (line, want) in [
        (
            r#"{"type":"system","uuid":"zz1","subtype":"zz_future_subtype","content":"zzprobe"}"#,
            "system/zz_future_subtype",
        ),
        (
            r#"{"type":"attachment","uuid":"zz1","attachment":{"type":"zz_future_payload"}}"#,
            "attachment/zz_future_payload",
        ),
        (
            r#"{"type":"zz-future-line","zzprobe":"x"}"#,
            "zz-future-line",
        ),
    ] {
        let rec: Record = serde_json::from_str(line).expect("valid record");
        assert!(
            rec.classify(&ClassifyCtx::top_level()).is_empty(),
            "{want} carries a leaf: {:?}",
            rec.classify(&ClassifyCtx::top_level())
        );
        assert_eq!(
            crate::commands::search::unmodeled_shape(&rec, Some(line)).as_deref(),
            Some(want),
            "the unit must name the value csift has not modeled"
        );
    }

    // The FOUR rows that carry no leaf and are not unmodeled either: their leaf is decided
    // on a deeper axis, which is why a selector reaching that deeper leaf admits the line.
    // The table's `columns` test holds their gates; this names the set, so a fifth leafless
    // row appearing on the value axes fails here rather than passing as one of these.
    let deeper: Vec<&str> = rows()
        .filter(|r| r.leaf.is_none() && matches!(r.shape, Shape::TopLevelType(_)))
        .map(|r| r.value())
        .collect();
    assert_eq!(deeper, ["user", "assistant", "system", "attachment"]);
}

/// Every `NEVER_CONVERTS` payload reads `delivered: false` at the RECORD level.
///
/// The `llm` column says the same thing and is documentation: SPEC section 5 and both
/// SKILL tables print it, and the drift test beside this one holds it to the leaf default.
/// `Record::delivery_override` is what a BARE selector consults, so it is the answer that
/// decides whether a record reaches `-t agent` - and the two are separate code with no
/// shared source. A payload the converter drops while the override says nothing would be
/// a row that reads right and behaves wrong.
#[test]
fn every_never_converting_payload_is_not_delivered() {
    for v in NEVER_CONVERTS {
        let line = format!(r#"{{"type":"attachment","uuid":"zz1","attachment":{{"type":"{v}"}}}}"#);
        let rec: Record = serde_json::from_str(&line).expect("valid record");
        assert_eq!(
            rec.delivery_override(),
            Some(false),
            "{v} is a drop arm, so its record is not delivered"
        );
    }
    // And a payload the converter DOES convert carries NO override, so the leaf default
    // stands for it - which is what makes the assertion above a statement about the drop
    // arms rather than about every attachment line.
    let converted =
        r#"{"type":"attachment","uuid":"zz1","attachment":{"type":"file","zzprobe":"x"}}"#;
    let rec: Record = serde_json::from_str(converted).expect("valid record");
    assert_eq!(rec.delivery_override(), None);
}
